# Rustory Agent Guide

이 파일은 짧은 bootstrap map이다. 공통 실행 계약과 상세 운영 규칙은 repository-owned 문서가 소유한다.

## First Read

- 공통 하네스 인터페이스와 Rustory overlay: `docs/agent-harness.md`.
- 제품/운영 탐색: `docs/HANDOFF.md`; 명령 선언: `docs/REPO_MANIFEST.yaml`.
- 현재 작업: 활성 `docs/todo-*/spec.md`와 `open-questions.md`.

<!-- agent-harness-baseline:start -->
## Agent Harness Baseline (GPT-5.6)

Baseline ID: `openai-gpt-5.6-2026-07-11`.

- Source of truth: use the `openai-docs` skill and the official [latest model guide](https://developers.openai.com/api/docs/guides/latest-model) plus [prompting best practices](https://developers.openai.com/api/docs/guides/latest-model#prompting-best-practices) before changing OpenAI model, API, prompt, or agent guidance.
- Model target: when the task asks for the current or latest OpenAI baseline, use `gpt-5.6`. This is harness guidance, not proof that the application calls OpenAI; change runtime model strings only at an existing OpenAI integration point.
- Prompt budget: start with the smallest prompt and task-relevant tool set that reliably completes the work. Preserve project-specific constraints, remove redundant generic instructions, and add examples only for an observed failure.
- Request modes: for answer, explain, review, diagnose, or plan requests, inspect and report without implementation. For change, build, or fix requests, make the requested in-scope local changes and run relevant non-destructive validation.
- Permissions: reading, searching, editing in-scope files, and running non-destructive checks are pre-authorized for change tasks. Require confirmation for external writes not explicitly requested, destructive or irreversible actions, purchases or cost, secrets, or material scope expansion.
- Persistence: continue until the requested outcome is complete; do not stop after only analysis, a partial patch, or an intermediate tool success. Stop and escalate only at a real permission, product-decision, or external-state boundary.
- Verification: treat tool and patch success as provisional. Re-read the diff and verify the user-visible or runtime outcome with the narrowest meaningful checks, then broaden only when risk warrants it.
- Publication boundary: before a public push, tag/release, visibility change, or published-history rewrite, run the repository boundary check and any authorized local private-inventory check. Keep private inventory outside published repositories and CI configuration; retain only non-identifying responsibility boundaries and operational contracts.
- Tracked-artifact privacy: treat tool output, memory-derived environment context, local absolute paths, machine/host/cluster identifiers, internal endpoints or addresses, and full diagnostic logs as local-only by default. Do not paste raw stdout or stderr into tracked files; retain repository-owned decisions and redacted verification outcomes with placeholders such as `<repo-root>`, `<private-host>`, `<internal-ip>`, and `<cluster-context>`.
- Output: lead with the conclusion. Include required evidence, material caveats, and the next action; trim introductions, repetition, generic reassurance, and optional background before trimming required content.
- Structure: use a lightweight task-specific plan or output shape. Do not impose a global template or long process narration when the repository already supplies the necessary workflow.
- Modes and orchestration: configure Pro mode in the API or runtime rather than asking the model to “think harder.” Use Programmatic Tool Calling only for bounded reduction stages with explicit schemas, limits, and no approval-sensitive side effects; keep semantic decisions and final validation direct.
- Evaluation: add or retain harness instructions only when repository checks or representative tasks show they improve final-answer completeness, evidence quality, reliability, latency, or cost. Evaluate the final result, not just tool-call count.
- Project overlay: the remaining sections of this file and the linked project docs define domain-specific architecture, tests, safety boundaries, escalation rules, and publish gates. They may specialize this baseline but must not silently weaken its permission or evidence requirements.
<!-- agent-harness-baseline:end -->

## Project Overlay

- local-first/P2P 경계와 relay acceptance는 `docs/agent-harness.md`와 제품 source를 따른다.
- background daemon spawn과 PID persistence는 하나의 성공 단위로 다룬다. PID 기록 또는 startup 확인이 실패하면 child를 종료·회수하고 restart 성공을 보고하지 않으며, Rust updater와 Python installer의 fallback 동작을 함께 검증한다.
- 기본 검증은 `scripts/check.sh --fast`; 출고 경계는 `docs/CHANGE_CONTROL.md`를 따른다.
- 로컬 VCS는 `jj`를 사용하고 push는 명시적 권한이 있을 때만 수행한다.
