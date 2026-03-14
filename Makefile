VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
TAG := v$(VERSION)

.PHONY: release check-version check-clean

## Tag and push a release. Fails if tag doesn't match Cargo.toml version or tree is dirty.
release: check-version check-clean
	git tag $(TAG)
	git push origin $(TAG)
	@echo "Released $(TAG)"

check-version:
	@if git rev-parse $(TAG) >/dev/null 2>&1; then \
		echo "Error: tag $(TAG) already exists"; \
		exit 1; \
	fi
	@echo "Version: $(VERSION) -> tag: $(TAG)"

check-clean:
	@if [ -n "$$(git status --porcelain)" ]; then \
		echo "Error: working tree is dirty. Commit or stash changes first."; \
		exit 1; \
	fi
