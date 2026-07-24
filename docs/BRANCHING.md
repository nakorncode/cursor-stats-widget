# Branch & release workflow

## Branches

| Branch | Role |
|--------|------|
| `main` | Stable / release line. Tags (`v*`) are cut from here. |
| `develop` | Day-to-day work. |

## Start / resume development (always)

```powershell
cd G:\NakornCode\git\cursor-stats-widget
git fetch origin
git checkout develop
git rebase origin/main
# if conflicts: fix, git add, git rebase --continue
```

Or:

```powershell
.\scripts\dev-sync.ps1
```

## Release a new version

1. Finish work on `develop` (commit + push).
2. **Always merge into `main` first** (no releasing from `develop` alone):

```powershell
git fetch origin
git checkout main
git pull origin main
git merge origin/develop
# bump version in package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml
git add -A
git commit -m "chore: bump version to x.y.z"
git push origin main
```

3. Tag and push (CI builds NSIS + MSI and publishes the GitHub Release):

```powershell
git tag -a vx.y.z -m "vx.y.z"
git push origin vx.y.z
```

4. Sync `develop` back so it includes the release commit:

```powershell
git checkout develop
git rebase origin/main
git push origin develop
```

## Notes

- Do not force-push `main`.
- Prefer `rebase` onto `main` for `develop` (linear history).
- Release CI: `.github/workflows/release.yml` (runs on `v*` tags).
