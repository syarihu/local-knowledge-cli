#!/usr/bin/env python3
"""Laya MLX Unix Domain Socket server for local-knowledge-cli (lk).

Provides fast, low-latency typed decisions (duplicate detection,
keyword filtering, and search reranking) over a local Unix domain socket.
Includes an idle shutdown timer that automatically terminates the daemon
after a period of inactivity to release memory.
"""

import argparse
import asyncio
import json
import os
import signal
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, Dict, List, Optional

try:
    import laya_mlx as laya
except ImportError:
    print("Error: laya-mlx is required. Run with `uv run --with laya-mlx python3 ...`", file=sys.stderr)
    sys.exit(1)

DEFAULT_MODEL = "aac6fef/laya-multilingual-mlx"
DEFAULT_IDLE_TIMEOUT = 600  # 10 minutes


class LayaServer:
    def __init__(
        self,
        socket_path: Path,
        model_name: str = DEFAULT_MODEL,
        idle_timeout: int = DEFAULT_IDLE_TIMEOUT,
    ):
        self.socket_path = socket_path
        self.model_name = model_name
        self.idle_timeout = idle_timeout
        self.last_active_time = time.time()
        self.agent: Optional[Any] = None
        self.server: Optional[asyncio.Server] = None
        self._running = True
        self.executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="laya-worker")

    def load_model(self) -> None:
        print(f"Loading Laya model: {self.model_name}...", file=sys.stderr)
        start = time.time()
        self.agent = laya.load(self.model_name)
        elapsed = time.time() - start
        print(f"Model loaded in {elapsed:.2f}s", file=sys.stderr)

    def handle_ping(self, _params: Dict[str, Any]) -> Dict[str, Any]:
        return {
            "status": "ok",
            "model": self.model_name,
            "idle_timeout": self.idle_timeout,
        }

    def handle_duplicate(self, params: Dict[str, Any]) -> Dict[str, Any]:
        assert self.agent is not None
        entry_a = params.get("entry_a", {})
        entry_b = params.get("entry_b", {})

        state = {
            "entry_a_title": entry_a.get("title", ""),
            "entry_a_content": entry_a.get("content", ""),
            "entry_b_title": entry_b.get("title", ""),
            "entry_b_content": entry_b.get("content", ""),
        }
        questions = {
            "is_duplicate": {
                "type": "noul",
                "instructions": (
                    "Do Entry A and Entry B describe the exact same technical specification, "
                    "policy, bug solution, or architectural decision?"
                ),
            }
        }
        res = self.agent.predict(state, questions)
        prob = float(res["answers"]["is_duplicate"]["noul"])
        threshold = float(params.get("threshold", 0.85))
        return {
            "is_duplicate": prob >= threshold,
            "probability": prob,
        }

    def handle_filter_keywords(self, params: Dict[str, Any]) -> Dict[str, Any]:
        assert self.agent is not None
        title = params.get("title", "")
        content = params.get("content", "")
        candidates: List[str] = params.get("candidates", [])

        if not candidates:
            return {"ranked_keywords": []}

        state = {
            "title": title,
            "content": content,
        }
        questions = {
            f"kw_{i}": {
                "type": "noul",
                "instructions": (
                    f"Is `{kw}` a core, essential technical keyword representing the main topic "
                    "in `title` and `content`?"
                ),
            }
            for i, kw in enumerate(candidates)
        }

        res = self.agent.predict(state, questions)
        ranked: List[Dict[str, Any]] = []
        for i, kw in enumerate(candidates):
            prob = float(res["answers"][f"kw_{i}"]["noul"])
            ranked.append({"keyword": kw, "score": prob})

        ranked.sort(key=lambda x: x["score"], reverse=True)
        return {"ranked_keywords": ranked}

    def handle_rerank(self, params: Dict[str, Any]) -> Dict[str, Any]:
        assert self.agent is not None
        query = params.get("query", "")
        candidates: List[Dict[str, Any]] = params.get("candidates", [])

        if not candidates:
            return {"scores": []}

        # Evaluate each candidate against the query
        scores = []
        for cand in candidates:
            cand_id = cand.get("id")
            cand_title = cand.get("title", "")
            cand_content = cand.get("content", "")

            state = {
                "query": query,
                "document_title": cand_title,
                "document_content": cand_content,
            }
            questions = {
                "relevance": {
                    "type": "noul",
                    "instructions": (
                        "Does the document described in `document_title` and `document_content` "
                        "contain relevant information answering or addressing the user search `query`?"
                    ),
                }
            }
            res = self.agent.predict(state, questions)
            score = float(res["answers"]["relevance"]["noul"])
            scores.append({"id": cand_id, "score": score})

        # Sort descending by score
        scores.sort(key=lambda x: x["score"], reverse=True)
        return {"scores": scores}

    def handle_categorize(self, params: Dict[str, Any]) -> Dict[str, Any]:
        assert self.agent is not None
        title = params.get("title", "")
        content = params.get("content", "")

        state = {
            "title": title,
            "content": content,
        }
        questions = {
            "category": {
                "type": "choice",
                "instructions": "Which category best fits the engineering note described in `title` and `content`?",
                "criteria": {
                    "bugs": "bug fixes, investigation of issues, errors, troubleshooting, regressions",
                    "architecture": "system architecture, high-level design, database schema, tech stack decisions",
                    "decisions": "ADR, architectural decision records, trade-off evaluations, rejected alternatives",
                    "conventions": "coding guidelines, naming rules, workflow rules, formatting",
                    "features": "new feature design, specifications, requirements, user stories",
                    "context": "conversation logs, session handoff context, temporary scratchpad notes",
                },
            }
        }
        res = self.agent.predict(state, questions)
        ans = res["answers"]["category"]
        return {
            "category": ans["choice"],
            "confidence": float(ans.get("confidence", 0.0)),
            "probabilities": {k: float(v) for k, v in ans.get("probabilities", {}).items()},
        }

    def dispatch(self, req: Dict[str, Any]) -> Dict[str, Any]:
        req_id = req.get("id")
        task = req.get("task", "")
        params = req.get("params", {})

        handlers = {
            "ping": self.handle_ping,
            "duplicate": self.handle_duplicate,
            "filter_keywords": self.handle_filter_keywords,
            "rerank": self.handle_rerank,
            "categorize": self.handle_categorize,
        }

        handler = handlers.get(task)
        if not handler:
            return {"id": req_id, "error": f"Unknown task: {task}"}

        try:
            result = handler(params)
            return {"id": req_id, "result": result}
        except Exception as e:
            return {"id": req_id, "error": str(e)}

    async def handle_client(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        while self._running:
            line = await reader.readline()
            if not line:
                break
            self.last_active_time = time.time()
            text = line.decode("utf-8").strip()
            if not text:
                continue

            try:
                req = json.loads(text)
            except json.JSONDecodeError as e:
                resp = {"id": None, "error": f"Invalid JSON: {e}"}
                writer.write((json.dumps(resp) + "\n").encode("utf-8"))
                await writer.drain()
                continue

            if req.get("task") == "shutdown":
                resp = {"id": req.get("id"), "result": {"status": "shutting_down"}}
                writer.write((json.dumps(resp) + "\n").encode("utf-8"))
                await writer.drain()
                self._running = False
                break

            # Handle ping immediately on event loop so liveness checks never wait behind inference
            if req.get("task") == "ping":
                resp = {"id": req.get("id"), "result": self.handle_ping(req.get("params", {}))}
            else:
                loop = asyncio.get_running_loop()
                resp = await loop.run_in_executor(self.executor, self.dispatch, req)

            self.last_active_time = time.time()
            writer.write((json.dumps(resp) + "\n").encode("utf-8"))
            await writer.drain()

        writer.close()
        await writer.wait_closed()

    async def idle_checker(self) -> None:
        while self._running:
            await asyncio.sleep(15)
            idle_seconds = time.time() - self.last_active_time
            if idle_seconds >= self.idle_timeout:
                print(
                    f"Idle timeout reached ({idle_seconds:.0f}s >= {self.idle_timeout}s). Shutting down...",
                    file=sys.stderr,
                )
                self._running = False
                if self.server:
                    self.server.close()
                break

    async def run(self) -> None:
        loop = asyncio.get_running_loop()
        await loop.run_in_executor(self.executor, self.load_model)

        # Ensure parent directory exists
        self.socket_path.parent.mkdir(parents=True, exist_ok=True)
        if self.socket_path.exists():
            try:
                self.socket_path.unlink()
            except OSError:
                pass

        pid_file = self.socket_path.with_suffix(".pid")
        pid_file.write_text(str(os.getpid()))

        self.server = await asyncio.start_unix_server(
            self.handle_client, path=str(self.socket_path)
        )
        print(f"Laya server listening on {self.socket_path} (pid: {os.getpid()})", file=sys.stderr)

        idle_task = asyncio.create_task(self.idle_checker())

        try:
            async with self.server:
                while self._running:
                    await asyncio.sleep(1)
        finally:
            idle_task.cancel()
            self.executor.shutdown(wait=False)
            if self.socket_path.exists():
                try:
                    self.socket_path.unlink()
                except OSError:
                    pass
            if pid_file.exists():
                try:
                    pid_file.unlink()
                except OSError:
                    pass
            print("Laya server shutdown complete.", file=sys.stderr)


def main() -> None:
    parser = argparse.ArgumentParser(description="Laya MLX Unix Domain Socket server for lk")
    parser.add_argument(
        "--socket-path",
        type=Path,
        default=Path.home() / ".cache" / "lk" / "laya.sock",
        help="Path to the Unix domain socket",
    )
    parser.add_argument(
        "--model",
        type=str,
        default=DEFAULT_MODEL,
        help="Laya model name on Hugging Face or local path",
    )
    parser.add_argument(
        "--idle-timeout",
        type=int,
        default=DEFAULT_IDLE_TIMEOUT,
        help="Seconds of inactivity before auto-shutdown",
    )
    args = parser.parse_args()

    server = LayaServer(
        socket_path=args.socket_path,
        model_name=args.model,
        idle_timeout=args.idle_timeout,
    )

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)

    def handle_sigterm() -> None:
        server._running = False
        if server.server:
            server.server.close()

    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, handle_sigterm)

    try:
        loop.run_until_complete(server.run())
    finally:
        loop.close()


if __name__ == "__main__":
    main()
