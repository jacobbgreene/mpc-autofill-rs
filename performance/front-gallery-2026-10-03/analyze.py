"""Summarize the fixed workload matrix without changing its acceptance criteria."""
import json
import math
from pathlib import Path
import random
import statistics as stats

root = Path(__file__).resolve().parent
data = json.loads((root / "final-results.json").read_text()) + json.loads((root / "followup-results.json").read_text())
rng = random.Random(20261003)
report = []

def percentile(values, fraction):
    values = sorted(values)
    return values[min(len(values) - 1, math.ceil(fraction * len(values)) - 1)]

for workload in ["normal", "demanding", "backs"]:
    for condition in ["cold", "warm"]:
        strata = {variant: sorted([row for row in data if row["variant"] == variant and row["workload"] == workload and row["condition"] == condition], key=lambda row: row["index"]) for variant in ["baseline", "candidate"]}
        summaries = {}
        for variant, rows in strata.items():
            durations = [row["durationMs"] for row in rows]
            frames = [frame for row in rows for frame in row["frames"]]
            summaries[variant] = {
                "n": len(rows), "median_ms": stats.median(durations),
                "cv": stats.stdev(durations) / stats.mean(durations),
                "min_ms": min(durations), "max_ms": max(durations),
                "frame_p95_ms": percentile(frames, .95),
                "frames_over_50": sum(frame > 50 for frame in frames) / len(frames),
                "cpu_median_ms": stats.median(row["taskMs"] for row in rows),
                "peak_rss": max(row["peakRss"] for row in rows),
                "peak_heap": max(row["peakHeap"] for row in rows),
            }
        baseline, candidate = summaries["baseline"], summaries["candidate"]
        pairs = list(zip(strata["baseline"], strata["candidate"]))
        deltas = [before["durationMs"] - after["durationMs"] for before, after in pairs]
        bootstrap = [stats.median(rng.choices(deltas, k=len(deltas))) for _ in range(10000)]
        low, high = percentile(bootstrap, .025), percentile(bootstrap, .975)
        improvement = baseline["median_ms"] - candidate["median_ms"]
        guards = {
            "warm_or_backs": condition == "cold" and workload != "backs" or -improvement <= max(.1 * baseline["median_ms"], 100),
            "frame_p95": candidate["frame_p95_ms"] <= max(baseline["frame_p95_ms"] * 1.25, 50),
            "long_frames": candidate["frames_over_50"] <= baseline["frames_over_50"] + .05,
            "cpu": candidate["cpu_median_ms"] <= max(baseline["cpu_median_ms"] * 1.25, baseline["cpu_median_ms"] + 100),
            "rss": candidate["peak_rss"] <= baseline["peak_rss"] + 160 * 1048576,
            "heap": candidate["peak_heap"] <= baseline["peak_heap"] + 32 * 1048576,
            "queues": all(row["pending"] == 0 and row["previewWaiting"] == 0 and row["peakPreview"] <= 8 and row["peakSearch"] <= 4 for row in strata["candidate"]),
            "dom": all(row["peakNodes"] <= len(row["seen"]) * 16 + 500 for row in strata["candidate"]),
            "request_totals": all(len(after["calls"]) <= len(before["calls"]) for before, after in pairs),
            "output_equivalence": all(before["chosen"] == after["chosen"] and before["seen"] == after["seen"] and before["finalName"] == after["finalName"] for before, after in pairs),
        }
        primary = improvement >= 100 and improvement >= .1 * baseline["median_ms"] and low > 0 if condition == "cold" and workload != "backs" else True
        report.append({"workload": workload, "condition": condition, "summary": summaries,
                       "improvement_ms": improvement, "improvement_percent": 100 * improvement / baseline["median_ms"],
                       "paired_improvement_bootstrap_95_ms": [low, high], "primary_pass": primary, "guardrails": guards})
(root / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
for row in report:
    print(row["workload"], row["condition"], round(row["improvement_percent"], 2), row["primary_pass"], all(row["guardrails"].values()))
