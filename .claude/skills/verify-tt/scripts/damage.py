#!/usr/bin/env python3
"""Copy a real ledger and damage the copy one way, so replay and dump can be
driven against it. The source ledger is never modified.

usage: damage.py <kind> <src.ledger> <dst.ledger>

kinds:
  torn      cut the main! root line in half
  no-root   delete the main! root line
  gap       delete the seq 2 line
  alien     rename seq 1 to hosted_file_delete, outside the boundary
  tamper    change the request of the first hosted_stdout_line crossing (Mismatch)
  drop-last delete the last crossing, so replay makes one the ledger lacks (Extra)
  add-entry repeat the last crossing as one more seq, which replay never makes (Missing)
  outcome   record the root's outcome as `returned 42` (Mismatch at seq 0)
  no-exe    delete the pinned binary
  no-spans  delete spans.jsonl
"""
import base64, json, os, shutil, sys

KINDS = ("torn", "no-root", "gap", "alien", "tamper", "drop-last", "add-entry", "outcome", "no-exe", "no-spans")


def span(line):
    return json.loads(line)["resourceSpans"][0]["scopeSpans"][0]["spans"][0]


def attrs(s):
    return {a["key"]: a["value"] for a in s["attributes"]}


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in KINDS:
        sys.exit(__doc__)
    kind, src, dst = sys.argv[1:]
    if os.path.exists(dst):
        sys.exit(f"damage.py: {dst} already exists")
    shutil.copytree(src, dst)
    spans_path = os.path.join(dst, "spans.jsonl")
    if kind == "no-exe":
        os.remove(os.path.join(dst, "exe"))
        return
    if kind == "no-spans":
        os.remove(spans_path)
        return

    lines = open(spans_path).read().splitlines()
    seqs = [int(attrs(span(l))["ledger.seq"]["intValue"]) for l in lines]
    root = seqs.index(0)
    if kind == "torn":
        lines[root] = lines[root][: len(lines[root]) // 2]
    elif kind == "no-root":
        del lines[root]
    elif kind == "gap":
        del lines[seqs.index(2)]
    elif kind == "drop-last":
        del lines[seqs.index(max(seqs))]
    elif kind == "add-entry":
        doc = json.loads(lines[seqs.index(max(seqs))])
        s = doc["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
        for a in s["attributes"]:
            if a["key"] == "ledger.seq":
                a["value"] = {"intValue": str(max(seqs) + 1)}
        lines.insert(root, json.dumps(doc, separators=(",", ":")))
    elif kind == "outcome":
        doc = json.loads(lines[root])
        s = doc["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
        for a in s["attributes"]:
            if a["key"] == "ledger.response":
                a["value"] = {"stringValue": base64.b64encode(b'{"returned":42}').decode()}
        lines[root] = json.dumps(doc, separators=(",", ":"))
    elif kind in ("alien", "tamper"):
        if kind == "alien":
            i = seqs.index(1)
        else:
            i = next(i for i, l in enumerate(lines) if span(l)["name"] == "hosted_stdout_line")
        doc = json.loads(lines[i])
        s = doc["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
        for a in s["attributes"]:
            if kind == "alien" and a["key"] == "ledger.effect":
                a["value"] = {"stringValue": "hosted_file_delete"}
            if kind == "tamper" and a["key"] == "ledger.request":
                a["value"] = {"stringValue": base64.b64encode(b"TAMPERED").decode()}
        if kind == "alien":
            s["name"] = "hosted_file_delete"
        lines[i] = json.dumps(doc, separators=(",", ":"))
    open(spans_path, "w").write("\n".join(lines) + "\n")


main()
