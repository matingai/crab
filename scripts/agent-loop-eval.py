#!/usr/bin/env python3
"""Run a small CLI eval suite for Crab's agent loop.

The suite focuses on routing and loop behavior:
- simple turns should use the small model and send no tool schemas;
- tool-grounded turns should stay on the main model and produce tool evidence;
- optional extended cases cover browser and computer-use surfaces.

Authentication is read from the existing environment, especially OPENAI_API_KEY.
The script never accepts an API key argument and never writes secrets to reports.
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any


@dataclasses.dataclass(frozen=True)
class EvalCase:
    name: str
    prompt: str
    suite: str = "core"
    expect_route: str | None = None
    response_contains: str | None = None
    min_tool_calls: int = 0
    required_tool: str | None = None
    max_iterations: int = 4
    enable_shell: bool = False


CASES: tuple[EvalCase, ...] = (
    EvalCase(
        name="direct_simple",
        prompt="Reply exactly: crab-eval-ok",
        expect_route="small",
        response_contains="crab-eval-ok",
        max_iterations=2,
    ),
    EvalCase(
        name="workspace_tool",
        prompt=(
            "Use the list_files tool to inspect the workspace root, then answer "
            "with exactly three top-level entries."
        ),
        expect_route="primary",
        min_tool_calls=1,
        required_tool="list_files",
        max_iterations=4,
    ),
    EvalCase(
        name="code_navigation",
        prompt=(
            "Use repository tools to find where smart model routing is implemented. "
            "Answer with the main file path and one relevant function name."
        ),
        expect_route="primary",
        min_tool_calls=1,
        response_contains="smart_model_routing",
        max_iterations=5,
    ),
    EvalCase(
        name="computer_use_status",
        suite="extended",
        prompt=(
            "Use the computer_use tool with a read-only status action, then answer "
            "whether computer control appears available."
        ),
        expect_route="primary",
        min_tool_calls=1,
        required_tool="computer_use",
        max_iterations=4,
    ),
    EvalCase(
        name="browser_local_page",
        suite="extended",
        prompt=(
            "Use browser_navigate to open https://example.com, then use a browser "
            "snapshot and answer with the page title."
        ),
        expect_route="primary",
        min_tool_calls=1,
        required_tool="browser_navigate",
        max_iterations=5,
    ),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default=os.environ.get("OPENAI_BASE_URL", "http://localhost:50930/v1"))
    parser.add_argument("--api-mode", default=os.environ.get("OPENAI_API_MODE", "responses"))
    parser.add_argument("--main-model", default=os.environ.get("HERMES_RS_MODEL", "gpt-5.5"))
    parser.add_argument("--small-model", default=os.environ.get("HERMES_RS_SMART_MODEL", "gpt-5.4-mini"))
    parser.add_argument(
        "--suite",
        choices=("core", "extended", "all"),
        default="core",
        help="core is stable; extended adds browser/computer-use cases.",
    )
    parser.add_argument(
        "--case",
        action="append",
        dest="case_names",
        help="Run only the named case. Can be passed more than once.",
    )
    parser.add_argument(
        "--command",
        default="cargo run --quiet --",
        help="Command prefix for the Crab CLI, for example 'target/debug/crab'.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Directory for JSON/Markdown reports. Defaults to target/agent-loop-eval/<timestamp>.",
    )
    parser.add_argument("--timeout", type=int, default=180, help="Per-case timeout in seconds.")
    parser.add_argument("--preview-only", action="store_true", help="Prepare prompts without executing model calls.")
    return parser.parse_args()


def selected_cases(args: argparse.Namespace) -> list[EvalCase]:
    cases = list(CASES)
    if args.suite == "core":
        cases = [case for case in cases if case.suite == "core"]
    elif args.suite == "extended":
        cases = [case for case in cases if case.suite in {"core", "extended"}]

    if args.case_names:
        wanted = set(args.case_names)
        cases = [case for case in cases if case.name in wanted]
        missing = sorted(wanted.difference(case.name for case in cases))
        if missing:
            raise SystemExit(f"unknown eval case(s): {', '.join(missing)}")

    return cases


def make_output_dir(args: argparse.Namespace) -> Path:
    if args.output_dir:
        root = args.output_dir
    else:
        stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        root = Path("target") / "agent-loop-eval" / stamp
    root.mkdir(parents=True, exist_ok=True)
    return root.resolve()


def base_env(args: argparse.Namespace, data_dir: Path, case: EvalCase) -> dict[str, str]:
    env = os.environ.copy()
    env.update(
        {
            "OPENAI_BASE_URL": args.base_url,
            "OPENAI_API_MODE": args.api_mode,
            "HERMES_RS_API_MODE": args.api_mode,
            "HERMES_RS_MODEL": args.main_model,
            "HERMES_RS_AUX_MODEL": args.small_model,
            "HERMES_RS_AUX_API_MODE": args.api_mode,
            "HERMES_RS_DATA_DIR": str(data_dir),
            "HERMES_RS_DEBUG_CONTEXT": "1",
            "HERMES_RS_SMART_MODEL_ROUTING_ENABLED": "1",
            "HERMES_RS_SMART_MODEL": args.small_model,
            "HERMES_RS_SMART_MODEL_BASE_URL": args.base_url,
            "HERMES_RS_SMART_MODEL_API_KEY_ENV": "OPENAI_API_KEY",
            "HERMES_RS_SMART_MODEL_API_MODE": args.api_mode,
            "RUST_LOG": env.get("RUST_LOG", "warn"),
        }
    )
    if case.enable_shell:
        env["HERMES_RS_ENABLE_SHELL"] = "1"
    return env


def run_cli(
    args: argparse.Namespace,
    case: EvalCase,
    data_dir: Path,
    env: dict[str, str],
) -> tuple[subprocess.CompletedProcess[str], dict[str, Any] | None]:
    command = shlex.split(args.command)
    command.extend(["--max-iterations", str(case.max_iterations), "debug-context"])
    if args.preview_only:
        command.extend(["--prompt", case.prompt])
    else:
        command.extend(["--execute", "--events", "--prompt", case.prompt])

    proc = subprocess.run(
        command,
        cwd=Path.cwd(),
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=args.timeout,
    )
    payload = None
    if proc.stdout.strip():
        try:
            payload = json.loads(proc.stdout)
        except json.JSONDecodeError:
            payload = None
    return proc, payload


def collect_session(data_dir: Path, session_id: str | None) -> dict[str, Any] | None:
    if not session_id:
        return None
    path = data_dir / "sessions" / f"{session_id}.json"
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def collect_context_snapshots(data_dir: Path, session_id: str | None) -> list[dict[str, Any]]:
    if not session_id:
        return []
    directory = data_dir / "runtime" / "context-debug" / session_id
    if not directory.is_dir():
        return []
    snapshots: list[dict[str, Any]] = []
    for path in sorted(directory.glob("*.json")):
        try:
            item = json.loads(path.read_text(encoding="utf-8"))
            item["_file"] = str(path)
            snapshots.append(item)
        except json.JSONDecodeError:
            continue
    return snapshots


def event_items(payload: dict[str, Any] | None, event_type: str) -> list[dict[str, Any]]:
    events = payload.get("events", []) if payload else []
    return [event for event in events if event.get("type") == event_type]


def session_tool_names(session: dict[str, Any] | None) -> list[str]:
    if not session:
        return []
    names: list[str] = []
    for message in session.get("history", []):
        for call in message.get("tool_calls") or []:
            name = ((call.get("function") or {}).get("name") or "").strip()
            if name:
                names.append(name)
    return names


def evaluate_case(
    case: EvalCase,
    args: argparse.Namespace,
    proc: subprocess.CompletedProcess[str],
    payload: dict[str, Any] | None,
    session: dict[str, Any] | None,
    snapshots: list[dict[str, Any]],
) -> tuple[bool, list[str], dict[str, Any]]:
    reasons: list[str] = []
    preview = payload.get("preview", payload) if payload else {}
    response = (payload or {}).get("assistant_response", "")
    model_starts = event_items(payload, "model_request_started")
    model_finishes = event_items(payload, "model_request_finished")
    tool_finishes = event_items(payload, "tool_call_finished")
    tool_names = session_tool_names(session)
    assistant_snapshots = [item for item in snapshots if item.get("phase") == "assistant_request"]

    if proc.returncode != 0:
        reasons.append(f"process exited with {proc.returncode}")
    if payload is None:
        reasons.append("stdout was not valid JSON")

    routed_model = preview.get("routed_model")
    effective_tools = preview.get("effective_tool_definition_count")
    if case.expect_route == "small":
        if routed_model != args.small_model:
            reasons.append(f"expected small model {args.small_model}, saw {routed_model}")
        if effective_tools != 0:
            reasons.append(f"expected zero effective tools for small route, saw {effective_tools}")
    elif case.expect_route == "primary":
        first_model = model_starts[0].get("model") if model_starts else routed_model
        if first_model != args.main_model:
            reasons.append(f"expected primary model {args.main_model}, saw {first_model}")

    if (
        not args.preview_only
        and case.response_contains
        and case.response_contains.lower() not in response.lower()
    ):
        reasons.append(f"response did not contain {case.response_contains!r}")

    if not args.preview_only and len(tool_names) < case.min_tool_calls:
        reasons.append(f"expected at least {case.min_tool_calls} tool call(s), saw {len(tool_names)}")
    if not args.preview_only and case.required_tool and case.required_tool not in tool_names:
        reasons.append(f"required tool {case.required_tool!r} was not called")

    prompt_tokens = sum((event.get("prompt_tokens") or 0) for event in model_finishes)
    completion_tokens = sum((event.get("completion_tokens") or 0) for event in model_finishes)
    total_tokens = sum((event.get("total_tokens") or 0) for event in model_finishes)
    if total_tokens == 0:
        total_tokens = None

    metrics = {
        "name": case.name,
        "ok": not reasons,
        "reasons": reasons,
        "routed_model": routed_model,
        "routed_api_mode": preview.get("routed_api_mode"),
        "tool_definition_count": preview.get("tool_definition_count"),
        "effective_tool_definition_count": effective_tools,
        "projected_tokens": preview.get("projected_tokens"),
        "request_budget_tokens": preview.get("request_budget_tokens"),
        "model_request_count": len(model_starts),
        "models_seen": [event.get("model") for event in model_starts],
        "api_modes_seen": [event.get("api_mode") for event in model_starts],
        "tool_call_count": len(tool_names),
        "tool_names": tool_names,
        "tool_finish_count": len(tool_finishes),
        "prompt_tokens": prompt_tokens or None,
        "completion_tokens": completion_tokens or None,
        "total_tokens": total_tokens,
        "assistant_snapshot_tool_counts": [item.get("tool_count") for item in assistant_snapshots],
        "assistant_response_preview": response[:500],
    }
    return not reasons, reasons, metrics


def write_reports(root: Path, results: list[dict[str, Any]]) -> None:
    (root / "summary.json").write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")

    lines = [
        "# Crab Agent Loop Eval",
        "",
        f"- Cases: {len(results)}",
        f"- Passed: {sum(1 for item in results if item['ok'])}",
        f"- Failed: {sum(1 for item in results if not item['ok'])}",
        "",
        "| Case | OK | Routed model | Effective schemas | Tool calls | Projected tokens | Provider tokens | Notes |",
        "| --- | --- | --- | ---: | ---: | ---: | ---: | --- |",
    ]
    for item in results:
        notes = "; ".join(item["reasons"]) if item["reasons"] else ""
        provider_tokens = item["total_tokens"] if item["total_tokens"] is not None else ""
        lines.append(
            "| {name} | {ok} | {model} | {schemas} | {tools} | {projected} | {provider_tokens} | {notes} |".format(
                name=item["name"],
                ok="yes" if item["ok"] else "no",
                model=item.get("routed_model") or "",
                schemas=item.get("effective_tool_definition_count")
                if item.get("effective_tool_definition_count") is not None
                else "",
                tools=item.get("tool_call_count") or 0,
                projected=item.get("projected_tokens") or "",
                provider_tokens=provider_tokens,
                notes=notes.replace("|", "\\|"),
            )
        )
    lines.append("")
    (root / "summary.md").write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    args = parse_args()
    cases = selected_cases(args)
    root = make_output_dir(args)
    results: list[dict[str, Any]] = []

    print(f"Writing eval artifacts to {root}")
    for case in cases:
        print(f"==> {case.name}", flush=True)
        case_root = root / case.name
        data_dir = case_root / "data"
        case_root.mkdir(parents=True, exist_ok=True)
        env = base_env(args, data_dir, case)

        try:
            proc, payload = run_cli(args, case, data_dir, env)
        except subprocess.TimeoutExpired as error:
            result = {
                "name": case.name,
                "ok": False,
                "reasons": [f"timed out after {args.timeout}s"],
                "stdout": (error.stdout or "")[:1000],
                "stderr": (error.stderr or "")[:1000],
            }
            results.append(result)
            continue

        (case_root / "stdout.json").write_text(proc.stdout, encoding="utf-8")
        (case_root / "stderr.log").write_text(proc.stderr, encoding="utf-8")
        preview = payload.get("preview", payload) if payload else {}
        session_id = preview.get("session_id") if isinstance(preview, dict) else None
        session = collect_session(data_dir, session_id)
        snapshots = collect_context_snapshots(data_dir, session_id)
        ok, _reasons, metrics = evaluate_case(case, args, proc, payload, session, snapshots)
        metrics["data_dir"] = str(data_dir)
        metrics["session_id"] = session_id
        metrics["stderr_preview"] = proc.stderr[-1000:]
        results.append(metrics)
        status = "PASS" if ok else "FAIL"
        print(
            "    {status}: model={model} schemas={schemas} tools={tools}".format(
                status=status,
                model=metrics.get("routed_model"),
                schemas=metrics.get("effective_tool_definition_count"),
                tools=metrics.get("tool_call_count"),
            )
        )

    write_reports(root, results)
    failed = [item for item in results if not item.get("ok")]
    print(f"\nReport: {root / 'summary.md'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
