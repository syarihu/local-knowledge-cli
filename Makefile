VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
TAG := v$(VERSION)
REPO := syarihu/local-knowledge-cli
TAP_FORMULA := /Users/syarihu/git/syarihu/homebrew-tap/Formula/lk.rb

.PHONY: release check-version check-clean update-formula

## Tag and push a release. Fails if tag doesn't match Cargo.toml version or tree is dirty.
release: check-version check-clean
	git tag $(TAG)
	git push origin $(TAG)
	@echo "Released $(TAG)"

## Update Homebrew formula after a release. Fetches checksums and updates version + SHA256.
update-formula:
	@echo "Fetching checksums for $(TAG)..."
	@gh release download $(TAG) --repo $(REPO) --pattern checksums.txt --output /tmp/lk-checksums.txt || \
		(echo "Error: checksums.txt not found for $(TAG). Is the release ready?" && exit 1)
	@SHA_AARCH64_DARWIN=$$(grep aarch64-apple-darwin /tmp/lk-checksums.txt | awk '{print $$1}') && \
	SHA_X86_64_DARWIN=$$(grep x86_64-apple-darwin /tmp/lk-checksums.txt | awk '{print $$1}') && \
	SHA_AARCH64_LINUX=$$(grep aarch64-unknown-linux /tmp/lk-checksums.txt | awk '{print $$1}') && \
	SHA_X86_64_LINUX=$$(grep x86_64-unknown-linux /tmp/lk-checksums.txt | awk '{print $$1}') && \
	sed -i '' 's/version ".*"/version "$(VERSION)"/' $(TAP_FORMULA) && \
	awk -v s1="$$SHA_AARCH64_DARWIN" -v s2="$$SHA_X86_64_DARWIN" \
	    -v s3="$$SHA_AARCH64_LINUX" -v s4="$$SHA_X86_64_LINUX" ' \
	  /aarch64-apple-darwin/  { found="darwin_arm" } \
	  /x86_64-apple-darwin/   { found="darwin_x86" } \
	  /aarch64-unknown-linux/ { found="linux_arm" } \
	  /x86_64-unknown-linux/  { found="linux_x86" } \
	  /sha256/ && found=="darwin_arm"  { sub(/sha256 ".*"/, "sha256 \"" s1 "\""); found="" } \
	  /sha256/ && found=="darwin_x86"  { sub(/sha256 ".*"/, "sha256 \"" s2 "\""); found="" } \
	  /sha256/ && found=="linux_arm"   { sub(/sha256 ".*"/, "sha256 \"" s3 "\""); found="" } \
	  /sha256/ && found=="linux_x86"   { sub(/sha256 ".*"/, "sha256 \"" s4 "\""); found="" } \
	  { print }' $(TAP_FORMULA) > $(TAP_FORMULA).tmp && mv $(TAP_FORMULA).tmp $(TAP_FORMULA) && \
	echo "Updated $(TAP_FORMULA) to $(VERSION)" && \
	echo "  aarch64-apple-darwin: $$SHA_AARCH64_DARWIN" && \
	echo "  x86_64-apple-darwin:  $$SHA_X86_64_DARWIN" && \
	echo "  aarch64-linux:        $$SHA_AARCH64_LINUX" && \
	echo "  x86_64-linux:         $$SHA_X86_64_LINUX" && \
	echo "" && \
	echo "Don't forget to commit and push homebrew-tap!"
	@rm -f /tmp/lk-checksums.txt

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
