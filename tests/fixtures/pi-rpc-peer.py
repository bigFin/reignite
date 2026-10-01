# Test-only native RPC peer. All files and data are disposable.
import json
import os
import pathlib
import sys
import time

args = sys.argv[1:]
assert args[args.index("--mode") + 1] == "rpc"
session = pathlib.Path(args[args.index("--session") + 1])
case = args[args.index("--model") + 1]
root = session.parent
(root / "peer-pid").write_text(str(os.getpid()))
(root / "peer-argv").write_text(json.dumps(args))
rows = [json.loads(line) for line in session.read_text().splitlines()]
header, leaf = rows[0], rows[-1]["id"]
private = "PRIVATE-PI-RPC-CANARY"


def emit(value):
    encoded = json.dumps(value, ensure_ascii=False).encode() + b"\n"
    if case == "fragmented":
        for index in range(0, len(encoded), 3):
            sys.stdout.buffer.write(encoded[index:index + 3])
            sys.stdout.buffer.flush()
    else:
        sys.stdout.buffer.write(encoded)
        sys.stdout.buffer.flush()


def response(command, data):
    emit({"type": "response", "id": command["id"], "command": command["type"], "success": True, "data": data})


try:
    for line in sys.stdin.buffer:
        command = json.loads(line)
        with (root / "peer-requests").open("a") as log:
            log.write(json.dumps(command) + "\n")
        kind = command["type"]
        if kind == "get_state":
            if case == "timeout":
                time.sleep(5)
                continue
            if case == "trickle":
                for _ in range(100):
                    emit({"type": "thinking_level_changed", "level": "off"})
                    time.sleep(.02)
                continue
            if case == "corrupt":
                sys.stdout.write("not JSON " + private + "\n")
                sys.stdout.flush()
                continue
            if case == "partial":
                sys.stdout.write('{"type": "response"')
                sys.stdout.flush()
                break
            if case == "oversized":
                emit({"type": "thinking_level_changed", "private": "x" * (1024 * 1024 + 1)})
                continue
            if case == "stderr_flood":
                os.write(2, b"x" * (256 * 1024 + 1))
                continue
            if case == "startup_work":
                emit({"type": "agent_start"})
            if case == "dialog":
                for method in ("select", "confirm", "input", "editor"):
                    emit({"type": "extension_ui_request", "id": "dialog-" + method,
                          "method": method, "title": private})
            if case == "unknown_ui":
                emit({"type": "extension_ui_request", "id": "unknown", "method": "future-dialog", "title": private})
            if case == "wrong_correlation":
                command = {**command, "id": "foreign"}
            if case == "rejected":
                emit({"type": "response", "id": command["id"], "command": kind, "success": False, "error": private})
                continue
            response(command, {"sessionId": "foreign-session" if case == "wrong_session" else header["id"],
                               "sessionFile": str(root / "foreign.jsonl") if case == "wrong_file" else str(session),
                               "isStreaming": case == "busy", "isCompacting": False, "pendingMessageCount": 0,
                               "private": private + "\u2028" + private + "\u2029"})
        elif kind == "get_entries":
            response(command, {"entries": [], "leafId": "foreign-leaf" if case == "wrong_branch" else leaf})
        elif kind == "get_messages":
            if case == "changed_file":
                with session.open("a") as file:
                    file.write(json.dumps({"type": "message", "id": "other-leaf", "parentId": leaf,
                                           "message": {"role": "user", "content": private}}) + "\n")
            if case in ("disable_after_load", "expire_after_load"):
                state = root / "state/state.json"
                database = json.loads(state.read_text())
                if case == "disable_after_load":
                    database["policy"]["host_enabled"] = False
                else:
                    for record in database["records"].values():
                        record["attempt"]["expires"] = 0
                replacement = state.with_suffix(".replacement")
                replacement.write_text(json.dumps(database))
                replacement.chmod(0o600)
                replacement.replace(state)
            messages = [{"role": "user", "content": private}]
            if case == "pending_tool":
                messages = [{"role": "assistant", "content": [{"type": "toolCall", "id": "tool-1"}]},
                            {"role": "user", "content": private}]
            if case == "tool_error":
                messages = [{"role": "assistant", "content": [{"type": "toolCall", "id": "tool-1"}]},
                            {"role": "toolResult", "toolCallId": "tool-1", "isError": True, "content": private}]
            if case == "history_question":
                messages = [{"role": "assistant", "stopReason": "stop", "content": private}]
            response(command, {"messages": messages})
        elif kind == "prompt":
            assert "The host restarted" in command["message"]
            assert "Unknown tool delivery" in command["message"]
            database = json.loads((root / "state/state.json").read_text())
            assert all(record["attempt"]["status"] == "claimed" for record in database["records"].values())
            if case == "lost_ack":
                break
            if case == "prompt_dialog":
                emit({"type": "extension_ui_request", "method": "confirm", "id": "approval", "title": private})
                continue
            response(command, {"disposition": "queued" if case == "queued" else "handled" if case == "handled" else "started"})
            if case in ("queued", "handled"):
                continue
            emit({"type": "agent_start"})
            emit({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": private}})
            emit({"type": "agent_end", "willRetry": case == "end_only", "messages": []})
            if case == "end_only":
                continue
            if case == "run_dialog":
                emit({"type": "extension_ui_request", "method": "input", "id": "choice", "title": private})
                continue
            emit({"type": "message_end", "message": {"role": "assistant", "stopReason": "error" if case == "run_error" else "stop", "content": private}})
            emit({"type": "agent_settled"})
        else:
            raise AssertionError("unexpected command or approval answer: " + kind)
finally:
    (root / "peer-closed").write_text("closed")
