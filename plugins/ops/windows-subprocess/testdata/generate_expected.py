"""Generate parity outputs from the owner vault's windows_subprocess.py module.

The Python function branches on os.name, so each case patches os.name for the
call. Where the host lacks subprocess.STARTUPINFO (non-Windows), a stand-in
with the same two attributes is installed for the call. The returned
STARTUPINFO object is normalized to its dwFlags and wShowWindow fields.
"""

import importlib.util
import json
import subprocess
from pathlib import Path
from unittest import mock


TESTDATA = Path(__file__).resolve().parent
SOURCE = (
    Path.home()
    / "Projects"
    / "Notes"
    / "obsidian-note"
    / "4_Project"
    / "Multica-Agent-Strategy"
    / "Scripts"
    / "windows_subprocess.py"
)


class _StartupInfo:
    def __init__(self):
        self.dwFlags = 0
        self.wShowWindow = 0


def load_module():
    spec = importlib.util.spec_from_file_location("windows_subprocess", SOURCE)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load source module: {SOURCE}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def output_for(module, request):
    os_name = "nt" if request["is_windows"] else "posix"
    startupinfo_cls = getattr(subprocess, "STARTUPINFO", _StartupInfo)
    with mock.patch.object(module.os, "name", os_name), mock.patch.object(
        subprocess, "STARTUPINFO", startupinfo_cls, create=True
    ), mock.patch.object(subprocess, "STARTF_USESHOWWINDOW", 1, create=True):
        kwargs = module.hidden_subprocess_kwargs(
            new_process_group=request.get("new_process_group", False)
        )
    info = kwargs.get("startupinfo")
    return {
        "creationflags": kwargs.get("creationflags"),
        "startupinfo": None
        if info is None
        else {"dwFlags": int(info.dwFlags), "wShowWindow": int(info.wShowWindow)},
    }


def main():
    module = load_module()
    for input_path in sorted(TESTDATA.glob("*.input.json")):
        request = json.loads(input_path.read_text(encoding="utf-8"))
        output = output_for(module, request)
        expected_path = input_path.with_name(
            input_path.name.removesuffix(".input.json") + ".expected.json"
        )
        expected_path.write_text(
            json.dumps(output, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        print(f"generated {expected_path.name}")


if __name__ == "__main__":
    main()
