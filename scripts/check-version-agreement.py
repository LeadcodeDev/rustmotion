import sys, tomllib, pathlib

root = tomllib.loads(pathlib.Path("Cargo.toml").read_text())
ws = root["workspace"]
want = ws["package"]["version"]
bad = []

for name, spec in ws.get("dependencies", {}).items():
    if isinstance(spec, dict) and "path" in spec:
        got = spec.get("version")
        if got != want:
            bad.append(f"  [workspace.dependencies] {name}: {got!r} != {want!r}")

for m in ws["members"]:
    p = pathlib.Path(m) / "Cargo.toml"
    pkg = tomllib.loads(p.read_text())["package"]
    v = pkg.get("version")
    if v != {"workspace": True}:
        bad.append(f"  {p}: version is {v!r}, expected inherited from the workspace")

if bad:
    print(f"Version drift — the workspace declares {want}:")
    print("\n".join(bad))
    print("\nEvery crate and every internal dependency moves together; change it in\n[workspace.package] and let the rest inherit.")
    sys.exit(1)
print(f"All crates and internal dependencies agree on {want}")
