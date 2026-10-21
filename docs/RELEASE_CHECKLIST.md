# Release checklist

A release is published only when every automated gate is green and every manual item is either
done or listed as not done in the release notes. Version numbers come from `src-tauri/tauri.conf.json`
(kept equal in `Cargo.toml`, `package.json` and `ui/package.json`); the release workflow tags the
commit that produced the installers.

## Automated (CI must be green on the release commit)

- [ ] `CI` on Windows, macOS and Linux: Rust tests (including `file_recovery`, `migrations`,
      `dj_apps` round trips), UI tests, type check, clippy, installer smoke test (install, launch,
      analysis, model, playback in the WebView).
- [ ] `dependency-audit`: `cargo audit` and `npm audit` without new advisories (exceptions are
      listed with a reason in `ci.yml`).
- [ ] `Analyser evaluation` when analysis code changed: numbers in `AI_VALIDATION.md` updated.
- [ ] `Release` builds all installers; `checksums` attaches `SHA256SUMS.txt`.

## Manual (record the result in the release notes)

- [ ] Install from the published installer on Windows and macOS (Apple silicon), import a folder,
      play, decide, edit, undo, restart: settings and decisions persist.
- [ ] Export to at least Rekordbox XML and import it into Rekordbox; note any field that differs
      (see `TEST_PLAN.md`). Until this is done for an application, it is not called “verified”.
- [ ] Listening check on real speakers/headphones (preview, seek, hot region).
- [ ] Website download buttons point to the new version (they read the GitHub release).

## Documentation

- [ ] `STATUS.md`, `AUDIT_RESPONSE.md` and the release notes describe the same state.
- [ ] New or changed measurements are in `AI_VALIDATION.md`; claims on the website and README
      match them.
