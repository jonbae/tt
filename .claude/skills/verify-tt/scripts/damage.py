#!/usr/bin/env python3
"""Copy a real ledger and damage the copy one way, so replay and dump can be
driven against it. The source ledger is never modified.

usage: damage.py <kind> <src.ledger> <dst.ledger>

kinds:
  torn        cut the main! root line in half
  no-root     delete the main! root line
  gap         delete the seq 2 line
  alien       rename seq 1 to hosted_file_delete, outside the boundary
  tamper      change the request of the first hosted_stdout_line crossing (Mismatch)
  drop-last   delete the last crossing, so replay makes one the ledger lacks (Extra)
  add-entry   repeat the last crossing as one more seq, which replay never makes (Missing)
  outcome     record the root's outcome as `returned 42` (Mismatch at seq 0)
  no-seq      delete the ledger.seq attribute from the seq 1 line
  bad-b64     set seq 1's request to text that isn't base64
  bad-args    set the root's request to bytes that don't decode as process args
  bad-outcome set the root's response to JSON that isn't an outcome
  no-exe      delete the pinned binary
  no-spans    delete spans.jsonl
"""
import base64, json, os, shutil, sys

KINDS = ("torn", "no-root", "gap", "alien", "tamper", "drop-last", "add-entry", "outcome",
         "no-seq", "bad-b64", "bad-args", "bad-outcome", "no-exe", "no-spans")


def span(line):
    return json.loads(line)["resourceSpans"][0]["scopeSpans"][0]["spans"][0]


def attrs(s):
    return {a["key"]: a["value"] for a in s["attributes"]}


def b64(b):
    return base64.b64encode(b).decode()


def set_attr(lines, i, key, value):
    doc = json.loads(lines[i])
    s = doc["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
    s["attributes"] = [a for a in s["attributes"] if a["key"] != key]
    if value is not None:
        s["attributes"].append({"key": key, "value": {"stringValue": value}})
    lines[i] = json.dumps(doc, separators=(",", ":"))


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
        set_attr(lines, root, "ledger.response", b64(b'{"returned":42}'))
    elif kind == "no-seq":
        set_attr(lines, seqs.index(1), "ledger.seq", None)
    elif kind == "bad-b64":
        set_attr(lines, seqs.index(1), "ledger.request", "not base64!")
    elif kind == "bad-args":
        set_attr(lines, root, "ledger.request", b64(b"\xff\xff\xff\xff"))
    elif kind == "bad-outcome":
        set_attr(lines, root, "ledger.response", b64(b"{}"))
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
                a["value"] = {"stringValue": b64(b"TAMPERED")}
        if kind == "alien":
            s["name"] = "hosted_file_delete"
        lines[i] = json.dumps(doc, separators=(",", ":"))
    open(spans_path, "w").write("\n".join(lines) + "\n")


main()
