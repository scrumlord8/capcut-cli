"""JSON output envelope for agent-first CLI."""
import json
import sys
import time
from typing import Any, Optional, List


def success(command: str, data: Any, start_time: Optional[float] = None) -> dict:
    """Wrap a successful result in the standard JSON envelope."""
    envelope = {
        "status": "ok",
        "command": command,
        "data": data,
        "errors": [],
        "meta": {
            "version": "0.1.0",
        },
    }
    if start_time is not None:
        envelope["meta"]["duration_ms"] = int((time.time() - start_time) * 1000)
    return envelope


def error(command: str, code: str, message: str, hint: Optional[str] = None) -> dict:
    """Wrap an error in the standard JSON envelope."""
    err = {"code": code, "message": message}
    if hint:
        err["hint"] = hint
    return {
        "status": "error",
        "command": command,
        "data": None,
        "errors": [err],
        "meta": {"version": "0.1.0"},
    }


def emit(envelope: dict):
    """Print JSON envelope to stdout."""
    print(json.dumps(envelope, indent=2, default=str))


def log(msg: str):
    """Print a log message to stderr (not parsed by agents)."""
    print(msg, file=sys.stderr)
