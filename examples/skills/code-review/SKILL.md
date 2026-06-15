---
name: code-review
description: Code review guidelines and best practices. Use when asked to review code, check code quality, or evaluate pull requests.
license: MIT
metadata:
  author: raf-team
  version: "1.0"
---

# Code Review Skill

## Instructions

1. Use `read_file` to read the source files that need review.
2. Check for the following:
   - **Correctness**: Logic errors, edge cases, off-by-one errors.
   - **Security**: SQL injection, XSS, insecure deserialization, hardcoded secrets.
   - **Performance**: N+1 queries, unnecessary allocations, blocking calls in async context.
   - **Readability**: Clear naming, appropriate comments, consistent style.
3. For each issue found, provide:
   - File path and line number
   - Severity (critical / major / minor)
   - Description and suggested fix
4. Reference the style guide in `references/rust-guidelines.md` for Rust-specific conventions.
