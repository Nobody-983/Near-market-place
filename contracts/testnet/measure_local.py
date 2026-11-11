#!/usr/bin/env python3
"""Run the already-built offline VM probe, then validate derived candidates.

The ignored native test executes local Wasm through MockedExternal only. This
does not call near CLI, sign, submit transactions, deploy, or perform RPC reads.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def main():
    target = ROOT / "contracts/marketplace/runtime/target/debug/deps"
    binaries = [p for p in target.glob("preparation_measurements-*") if p.is_file() and os.access(p, os.X_OK)]
    if len(binaries) != 1:
        raise ValueError("Build the documented native harness; exactly one executable is required")

    def execute(input_name, output_name):
        env = os.environ.copy()
        env["NEAR_PREPARATION_INPUT"] = str(HERE / "public" / input_name)
        env["NEAR_PREPARATION_OUTPUT"] = str(HERE / "public" / output_name)
        subprocess.run([str(binaries[0]), "--ignored", "--exact", "measure_preparation"], env=env, check=True)

    subprocess.run([sys.executable, "-B", str(HERE / "measurement_input.py")], check=True)
    execute("probe-input.json", "probe-measurements.json")
    subprocess.run([sys.executable, "-B", str(HERE / "measurement_input.py"), "--measured",
                    str(HERE / "public/probe-measurements.json")], check=True)
    execute("measurement-input.json", "measurements.json")
    for name in ("probe-measurements.json", "measurements.json"):
        report = json.loads((HERE / "public" / name).read_text())
        for role in ("nft", "marketplace"):
            for row in report[role].values():
                row["records"].sort(key=lambda r: r["key_hex"])
        report["marketplace_records"].sort(key=lambda r: r["key"])
        (HERE / "public" / name).write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
