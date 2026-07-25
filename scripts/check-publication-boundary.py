#!/usr/bin/env python3
from __future__ import annotations

import argparse
import ipaddress
import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


LESSONS_PATHS = {
    "docs/LESSONS_ARCHIVE.md",
    "docs/LESSONS_LOG.md",
}

SAFE_HOME_USERS = {
    "example",
    "local-user",
    "me",
    "runner",
    "tester",
    "user",
    "you",
}

SAFE_KUBERNETES_CONTEXTS = {
    "example",
    "example-cluster",
    "kind",
    "minikube",
    "test",
    "test-cluster",
}

DOCUMENTATION_NETWORKS = tuple(
    ipaddress.ip_network(network)
    for network in ("192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24")
)

SAFE_NETWORK_LITERALS = {
    "10.0.0.0/8",
    "100.64.0.0/10",
    "172.16.0.0/12",
    "192.168.0.0/16",
}

RAW_EVIDENCE_NAME = (
    r"(?:[a-z0-9][a-z0-9._-]*[-_])?"
    r"(?:healthcheck|diagnostic|support-bundle|cluster-dump)[-_]"
    r"[0-9]{8}(?:[-_][0-9]{4,6})?"
)
RAW_EVIDENCE_PATH = re.compile(rf"(?i)(?:^|/){RAW_EVIDENCE_NAME}(?:/|$)")
RAW_EVIDENCE_REFERENCE = re.compile(
    rf"(?i)(?<![a-z0-9._/-]){RAW_EVIDENCE_NAME}(?:/|$)"
)


@dataclass(frozen=True, order=True)
class Finding:
    path: str
    line: int
    kind: str


def run_command(root: Path, command: list[str]) -> str:
    completed = subprocess.run(
        command,
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip().splitlines()
        summary = detail[-1] if detail else f"exit {completed.returncode}"
        raise RuntimeError(f"{command[0]} {command[1] if len(command) > 1 else ''} failed: {summary}")
    return completed.stdout


def run_git(root: Path, *args: str) -> str:
    return run_command(root, ["git", *args])


def run_git_bytes(root: Path, *args: str) -> bytes:
    completed = subprocess.run(
        ["git", *args],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.decode("utf-8", errors="replace").strip().splitlines()
        summary = detail[-1] if detail else f"exit {completed.returncode}"
        raise RuntimeError(f"git {args[0] if args else ''} failed: {summary}")
    return completed.stdout


def run_jj(root: Path, *args: str) -> str:
    return run_command(root, ["jj", *args])


def repository_root(cwd: Path) -> Path:
    try:
        return Path(run_git(cwd, "rev-parse", "--show-toplevel").strip())
    except RuntimeError:
        return Path(run_jj(cwd, "workspace", "root").strip())


def tracked_files(root: Path) -> list[str]:
    try:
        return [item for item in run_git(root, "ls-files", "-z").split("\0") if item]
    except RuntimeError:
        return [item for item in run_jj(root, "file", "list").splitlines() if item]


def repository_identity(root: Path) -> tuple[str, str]:
    try:
        remote = run_git(root, "config", "--get", "remote.origin.url").strip()
    except RuntimeError:
        git_root = Path(run_jj(root, "git", "root").strip())
        remote = run_command(root, ["git", "-C", str(git_root), "config", "--get", "remote.origin.url"]).strip()
    match = re.search(r"(?:github\.com[/:])([^/]+)/([^/#]+?)(?:\.git)?$", remote)
    if not match:
        raise RuntimeError("origin does not identify a GitHub owner/repository")
    return match.group(1), match.group(2)


def live_visibility() -> str | None:
    explicit = os.environ.get("PUBLICATION_LIVE_VISIBILITY", "").strip().lower()
    if explicit:
        if explicit in {"public", "private", "internal"}:
            return "public" if explicit == "public" else "internal"
        raise RuntimeError("PUBLICATION_LIVE_VISIBILITY must be public, private, or internal")

    event_path = os.environ.get("GITHUB_EVENT_PATH")
    if not event_path:
        return None
    payload = json.loads(Path(event_path).read_text(encoding="utf-8"))
    repository = payload.get("repository") or {}
    visibility = str(repository.get("visibility") or "").lower()
    if visibility:
        return "public" if visibility == "public" else "internal"
    if "private" in repository:
        return "internal" if repository["private"] else "public"
    return None


def declared_publication_class(document: str) -> str:
    matches = re.findall(r"^- Publication class: `(public|internal)`\.$", document, flags=re.MULTILINE)
    if len(matches) != 1:
        raise RuntimeError("docs/agent-harness.md must declare exactly one publication class")
    expected_check = "- Publication boundary check: `scripts/check-publication-boundary.py`."
    if document.count(expected_check) != 1:
        raise RuntimeError("docs/agent-harness.md must declare the canonical publication boundary check")
    return matches[0]


def publication_class(root: Path, target_revision: str | None = None) -> str:
    if target_revision is None:
        document = (root / "docs" / "agent-harness.md").read_text(encoding="utf-8")
    else:
        document = run_git(root, "show", f"{target_revision}:docs/agent-harness.md")
    return declared_publication_class(document)


def text_files(root: Path) -> Iterable[tuple[str, str]]:
    files = set(tracked_files(root))
    try:
        files.update(item for item in run_git(root, "ls-files", "--others", "--exclude-standard", "-z").split("\0") if item)
    except RuntimeError:
        pass
    for relative in sorted(files):
        path = root / relative
        if not path.is_file():
            continue
        data = path.read_bytes()
        if b"\0" in data:
            continue
        yield relative, data.decode("utf-8", errors="ignore")


def fixed_patterns(owner: str, repository: str) -> list[tuple[str, re.Pattern[str]]]:
    return [
        (
            "portfolio-disclosure",
            re.compile(
                r"(?i)(?:\b[0-9]+\s*(?:repositories|repos)\b|[0-9]+개\s*저장소|"
                r"all\s+repositories|cross-repository\s+agent-harness)"
            ),
        ),
        (
            "cross-repository-revision",
            re.compile(r"(?i)\b(?:gitops|rollout|cleanup|deployment)\s+(?:commit|revision|rev)\s+[`'\"]?[0-9a-f]{7,40}\b"),
        ),
        (
            "cross-repository-revision",
            re.compile(r"(?i)--(?:rollout|cleanup)-revision\s+[0-9a-f]{7,40}\b"),
        ),
        (
            "local-repository-state",
            re.compile(
                r"(?i)\b(?:companion|sibling)\b.{0,48}"
                r"\b(?:repo|repository)\b.{0,48}"
                r"\b(?:local|draft|branch|worktree)\b"
            ),
        ),
        (
            "same-owner-repository-url",
            re.compile(rf"(?i)(?:https?://github\.com/|git@github\.com:){re.escape(owner)}/(?!{re.escape(repository)}(?:\.git)?(?![A-Za-z0-9_.-]))[A-Za-z0-9_.-]+"),
        ),
        (
            "same-owner-repository-identity",
            re.compile(rf"(?i)(?<![A-Za-z0-9_./\\-]){re.escape(owner)}/(?!{re.escape(repository)}(?:\.git)?(?![A-Za-z0-9_.-]))[A-Za-z0-9_.-]+"),
        ),
    ]


def scan_text(
    relative: str,
    text: str,
    patterns: list[tuple[str, re.Pattern[str]]],
    top_levels: set[str],
    *,
    source_path: str | None = None,
) -> set[Finding]:
    findings: set[Finding] = set()
    classification_path = source_path or relative
    path_pattern = re.compile(r"(?<![A-Za-z0-9_.<>-])([A-Za-z0-9_.-]+)/(?=(?:apps|manifests|argocd|common|infra|deploy|charts)/)", re.IGNORECASE)
    home_pattern = re.compile(r"(?<![A-Za-z0-9_.-])/(?:Users|home)/([A-Za-z0-9._-]+)")
    windows_home_pattern = re.compile(r"(?i)(?<![A-Za-z0-9_.-])[A-Z]:\\Users\\([A-Za-z0-9._-]+)")
    ipv4_pattern = re.compile(r"(?<![0-9])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![0-9])")
    kubernetes_context_pattern = re.compile(r"(?i)--context(?:=|\s+)[`'\"]?([a-z0-9][a-z0-9._-]*)")
    private_hostname_pattern = re.compile(
        r"(?i)\b[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9-]+)*\."
        r"(?:local|internal|lan|home\.arpa|ts\.net)\b"
    )
    product_endpoint_pattern = re.compile(
        r"(?i)\brustory-(?:tracker|relay)\."
        r"(?!example(?:\.|\b)|test(?:\.|\b)|invalid(?:\.|\b))[A-Za-z0-9.-]+\b"
    )
    record_like = Path(classification_path).suffix.lower() in {".log", ".md", ".txt"}
    if RAW_EVIDENCE_PATH.search(classification_path):
        findings.add(Finding(relative, 1, "raw-runtime-evidence-path"))
    for match in RAW_EVIDENCE_REFERENCE.finditer(text):
        line_no = text.count("\n", 0, match.start()) + 1
        findings.add(Finding(relative, line_no, "raw-runtime-evidence-reference"))
    for kind, pattern in patterns:
        for match in pattern.finditer(text):
            line_no = text.count("\n", 0, match.start()) + 1
            findings.add(Finding(relative, line_no, kind))
    for line_no, line in enumerate(text.splitlines(), start=1):
        for match in path_pattern.finditer(line):
            if match.group(1) not in top_levels:
                findings.add(Finding(relative, line_no, "external-repository-path"))
        for match in home_pattern.finditer(line):
            if match.group(1).lower() not in SAFE_HOME_USERS:
                findings.add(Finding(relative, line_no, "machine-local-home-path"))
        for match in windows_home_pattern.finditer(line):
            if match.group(1).lower() not in SAFE_HOME_USERS:
                findings.add(Finding(relative, line_no, "machine-local-home-path"))
        for match in kubernetes_context_pattern.finditer(line):
            if match.group(1).lower() not in SAFE_KUBERNETES_CONTEXTS:
                findings.add(Finding(relative, line_no, "machine-kubernetes-context"))
        if record_like and private_hostname_pattern.search(line):
            findings.add(Finding(relative, line_no, "private-operations-hostname"))
        for match in product_endpoint_pattern.finditer(line):
            findings.add(Finding(relative, line_no, "private-operations-endpoint"))
        for match in ipv4_pattern.finditer(line):
            try:
                address = ipaddress.ip_address(match.group(0))
            except ValueError:
                continue
            suffix = re.match(r"/[0-9]{1,3}", line[match.end() :])
            if suffix and f"{address}{suffix.group(0)}" in SAFE_NETWORK_LITERALS:
                continue
            documentation_address = any(address in network for network in DOCUMENTATION_NETWORKS)
            if documentation_address or address.is_loopback or address.is_unspecified:
                continue
            if record_like:
                findings.add(Finding(relative, line_no, "specific-network-address"))
            elif address.is_global:
                findings.add(Finding(relative, line_no, "public-operations-address"))

    if classification_path in LESSONS_PATHS:
        lessons_patterns = [
            (
                "operational-inventory-count",
                re.compile(
                    r"(?i)(?:\b(?:fleet|cluster|k8s|kubernetes|argocd|nodes?|peers?|devices?|pods?|workloads?)\b|"
                    r"(?:노드|피어|장치|파드|워크로드|클러스터)).{0,60}\b[0-9]+\b|"
                    r"\b[0-9]+\b.{0,30}(?:노드|피어|장치|파드|워크로드)"
                ),
            ),
            (
                "operational-revision-evidence",
                re.compile(r"(?i)\b(?:release|rollout|deployment|릴리즈|배포).{0,40}\b[0-9a-f]{12,40}\b"),
            ),
            (
                "operational-checksum-evidence",
                re.compile(r"(?i)\b(?:sha-?256|checksum).{0,20}\b[0-9a-f]{32,64}\b"),
            ),
        ]
        for kind, pattern in lessons_patterns:
            for match in pattern.finditer(text):
                line_no = text.count("\n", 0, match.start()) + 1
                findings.add(Finding(relative, line_no, kind))
    return findings


def check_tree(root: Path, owner: str, repository: str) -> set[Finding]:
    top_levels = {path.split("/", 1)[0] for path in tracked_files(root)}
    patterns = fixed_patterns(owner, repository)
    findings: set[Finding] = set()
    for relative, text in text_files(root):
        findings.update(scan_text(relative, text, patterns, top_levels))
    return findings


def resolve_commit(root: Path, revision: str) -> str:
    resolved = run_git(root, "rev-parse", "--verify", f"{revision}^{{commit}}").strip()
    if not re.fullmatch(r"[0-9a-f]{40,64}", resolved):
        raise RuntimeError(f"revision did not resolve to an immutable commit: {revision}")
    return resolved


def revision_files(root: Path, revision: str) -> Iterable[tuple[str, str]]:
    tree = run_git_bytes(root, "ls-tree", "-r", "-z", "--full-tree", revision)
    for record in tree.split(b"\0"):
        if not record:
            continue
        metadata, separator, raw_path = record.partition(b"\t")
        if not separator:
            raise RuntimeError("unexpected git ls-tree record")
        fields = metadata.split()
        if len(fields) != 3 or fields[1] != b"blob":
            continue
        object_id = fields[2].decode("ascii")
        relative = raw_path.decode("utf-8", errors="surrogateescape")
        data = run_git_bytes(root, "cat-file", "blob", object_id)
        yield relative, data.decode("utf-8", errors="ignore")


def newly_reachable_blobs(
    root: Path, target_revision: str, base_revision: str | None
) -> Iterable[tuple[str, str, str]]:
    args = ["rev-list", "--objects", target_revision]
    if base_revision is not None:
        args.append(f"^{base_revision}")
    for line in run_git(root, *args).splitlines():
        object_id, _, path = line.partition(" ")
        if run_git(root, "cat-file", "-t", object_id).strip() != "blob":
            continue
        data = run_git_bytes(root, "cat-file", "blob", object_id)
        source_path = path or "<detached-blob>"
        label = f"{source_path}@{object_id[:12]}"
        yield source_path, label, data.decode("utf-8", errors="ignore")


def check_revision(
    root: Path,
    owner: str,
    repository: str,
    target_revision: str,
    base_revision: str | None,
) -> set[Finding]:
    target_paths = [relative for relative, _ in revision_files(root, target_revision)]
    top_levels = {path.split("/", 1)[0] for path in target_paths}
    patterns = fixed_patterns(owner, repository)
    findings: set[Finding] = set()
    for relative, text in revision_files(root, target_revision):
        findings.update(scan_text(relative, text, patterns, top_levels))
    for source_path, label, text in newly_reachable_blobs(
        root, target_revision, base_revision
    ):
        findings.update(
            scan_text(
                label,
                text,
                patterns,
                top_levels,
                source_path=source_path,
            )
        )
    return findings


def self_test() -> int:
    patterns = fixed_patterns("example", "public-app")
    top_levels = {"docs", "scripts", "src"}
    private_repository = "-".join(("private", "source"))
    private_revision = "".join(("dead", "beef"))
    local_state = " ".join(
        (
            "The companion platform",
            "repo currently has",
            "a local draft.",
        )
    )
    unix_home = "/" + "/".join(("Users", "local-account", "src", "public-app"))
    windows_home = "C:\\" + "\\".join(("Users", "local-account", "src", "public-app"))
    product_endpoint = "-".join(("rustory", "tracker")) + "." + ".".join(("private", "example", "net"))
    public_address = str(ipaddress.ip_address((8 << 24) | (8 << 16) | (8 << 8) | 8))
    private_address = str(ipaddress.ip_network("100.64.0.0/10").network_address + 10)
    private_context = "-".join(("private", "cluster"))
    private_hostname = ".".join(("node-a", "private", "internal"))
    evidence_path = f"cluster-healthcheck-{'0' * 8}-{'0' * 6}/SUMMARY.txt"
    inventory_count = 2 + 3
    deployment_revision = "".join(("01234567", "89abcdef"))
    unsafe = [
        ("fixture", f"See https://github.com/example/{private_repository} for details."),
        ("fixture", f"Apply {private_repository}" + "/apps/service/manifests."),
        ("fixture", f"GitOps revision {private_revision} was promoted."),
        ("fixture", local_state),
        ("fixture", f"Built from {unix_home}."),
        ("fixture", f"Built from {windows_home}."),
        ("fixture", f"Use {product_endpoint} for production."),
        ("fixture", f"The production endpoint is {public_address}."),
        ("docs/HANDOFF.md", f"The target was {private_address}."),
        ("docs/HANDOFF.md", f"Run kubectl --context {private_context} get pods."),
        ("docs/HANDOFF.md", f"Connect to {private_hostname}."),
        ("docs/report@2026.md", f"Connect to {private_hostname}."),
        (evidence_path, "ready"),
        ("docs/HANDOFF.md", f"Read {evidence_path} before publishing."),
        ("docs/LESSONS_LOG.md", f"The private fleet had {inventory_count} nodes ready."),
        ("docs/LESSONS_LOG.md", f"Release target {deployment_revision} was deployed."),
    ]
    safe = [
        ("fixture", "See https://github.com/example/public-app/releases."),
        ("fixture", "The private deployment source of truth owns promotion."),
        ("fixture", "Use docs/deploy/checklist.md for the local contract."),
        ("fixture", "Use <home>/<repo-root> and <private-host>."),
        ("fixture", "Use tracker.example.com and 192.0.2.10 in documentation."),
        ("docs/HANDOFF.md", "The shared carrier-grade network is 100.64.0.0/10."),
        ("docs/HANDOFF.md", "Run kubectl --context example-cluster get pods."),
        ("fixture", "A three-peer acceptance fixture passed."),
    ]
    if any(not scan_text(path, text, patterns, top_levels) for path, text in unsafe):
        print("self-test failed: expected unsafe fixture was not detected", file=sys.stderr)
        return 1
    if any(scan_text(path, text, patterns, top_levels) for path, text in safe):
        print("self-test failed: safe fixture was rejected", file=sys.stderr)
        return 1
    checker_path = Path(__file__)
    checker_findings = scan_text(
        "scripts/check-publication-boundary.py",
        checker_path.read_text(encoding="utf-8"),
        patterns,
        top_levels,
    )
    if checker_findings:
        print("self-test failed: checker source violates its own publication boundary", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory(prefix="publication-boundary-") as raw_dir:
        fixture = Path(raw_dir)
        run_git(fixture, "init", "-q")
        run_git(fixture, "config", "user.name", "Boundary Test")
        run_git(fixture, "config", "user.email", "boundary@example.invalid")
        run_git(fixture, "remote", "add", "origin", "https://github.com/example/public-app.git")
        (fixture / "README.md").write_text("safe\n", encoding="utf-8")
        run_git(fixture, "add", "README.md")
        run_git(fixture, "commit", "-qm", "base")
        base = resolve_commit(fixture, "HEAD")

        removed_secret = "-".join(("private", "source"))
        (fixture / "docs").mkdir()
        (fixture / "docs" / "report@2026.md").write_text(
            f"Connect to {private_hostname}.\n", encoding="utf-8"
        )
        (fixture / "removed.txt").write_text(
            f"https://github.com/example/{removed_secret}\n", encoding="utf-8"
        )
        run_git(fixture, "add", "docs/report@2026.md", "removed.txt")
        run_git(fixture, "commit", "-qm", "add removed sensitive blob")
        (fixture / "docs" / "report@2026.md").unlink()
        (fixture / "removed.txt").unlink()
        binary_secret = "-".join(("private", "binary"))
        (fixture / "artifact.bin").write_bytes(
            b"\0https://github.com/example/" + binary_secret.encode("ascii")
        )
        symlink_target = "/" + "/".join(("Users", "private-account", "secret"))
        (fixture / "link").symlink_to(symlink_target)
        run_git(fixture, "add", "-A")
        run_git(fixture, "commit", "-qm", "target")
        target = resolve_commit(fixture, "HEAD")
        revision_findings = check_revision(
            fixture, "example", "public-app", target, base
        )
        kinds = {finding.kind for finding in revision_findings}
        if (
            "same-owner-repository-url" not in kinds
            or "machine-local-home-path" not in kinds
            or "private-operations-hostname" not in kinds
        ):
            print(
                "self-test failed: revision/history binary or symlink fixture bypassed scanning",
                file=sys.stderr,
            )
            return 1
    print("publication boundary repository gate self-test passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate the repository-owned publication boundary contract.")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--stdin", action="store_true", help="scan candidate text from stdin instead of the working tree")
    parser.add_argument("--label", default="candidate", help="redacted location label used with --stdin")
    parser.add_argument("--target-rev", help="scan this immutable Git commit instead of the working tree")
    parser.add_argument("--base-rev", help="also scan blobs newly reachable after this Git commit")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    try:
        root = repository_root(Path.cwd())
        target_revision = resolve_commit(root, args.target_rev) if args.target_rev else None
        base_revision = resolve_commit(root, args.base_rev) if args.base_rev else None
        if base_revision and not target_revision:
            raise RuntimeError("--base-rev requires --target-rev")
        if base_revision and subprocess.run(
            ["git", "merge-base", "--is-ancestor", base_revision, target_revision],
            cwd=root,
            check=False,
        ).returncode != 0:
            raise RuntimeError("--base-rev must be an ancestor of --target-rev")

        declared = publication_class(root, target_revision)
        live = live_visibility()
        if live is not None and live != declared:
            print(
                f"publication boundary check failed: declared class {declared} does not match live class {live}",
                file=sys.stderr,
            )
            return 1
        if declared == "internal":
            print("publication boundary check passed: class=internal")
            return 0

        owner, repository = repository_identity(root)
        if args.stdin:
            top_levels = {path.split("/", 1)[0] for path in tracked_files(root)}
            findings = scan_text(args.label, sys.stdin.read(), fixed_patterns(owner, repository), top_levels)
        elif target_revision:
            findings = check_revision(
                root, owner, repository, target_revision, base_revision
            )
        else:
            findings = check_tree(root, owner, repository)
        if findings:
            for finding in sorted(findings):
                print(
                    f"publication boundary finding: path={finding.path} line={finding.line} class={finding.kind}",
                    file=sys.stderr,
                )
            print(f"publication boundary check failed: {len(findings)} redacted finding(s)", file=sys.stderr)
            return 1
        print("publication boundary check passed: class=public")
        return 0
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"publication boundary check could not prove safety: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
