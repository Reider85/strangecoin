[GITHUB-P23] release: re-tag v3.0.0-stage3 on post-UI-P02-C master HEAD

This commit repositions the v3.0.0-stage3 tag from the old commit 89dd756 (16 Nov, pre-UI-P02-C) to the current master HEAD (23f4641, 19 Nov) to resolve the release pipeline failure.

## Problem
- v3.0.0-stage3 tag was on commit 89dd756 where macOS builds failed due to objc2 0.3.0-beta.3 incompatibility
- Two release workflow runs (#12, #13) failed on this tag
- No GitHub Release was created despite the tag existing
- UI-P02-C fixes (18 Nov) resolved macOS build issues, but tag remained on old commit

## Solution
1. Deleted existing v3.0.0-stage3 tag locally and remotely
2. Created new annotated tag on current master HEAD (23f4641)
3. Pushed new tag to origin

## Verification Steps
1. Check tag repositioning:
   ```bash
   git ls-remote --tags origin | grep v3.0.0-stage3
   # Should show new SHA: a653a0d39ba6a9fed46fb0d2f31f5de741bccc0a
   ```

2. Monitor GitHub Actions → Release workflow for new run (should be #14)
   - Expected: 6 matrix cells with macOS allow_failure: true
   - Build time: ~18-25 minutes

3. Verify GitHub Release at /releases/tag/v3.0.0-stage3
   - Should show stable release (not pre-release)
   - Should contain 6 binaries + SHA256SUMS.txt + cosign signatures + SLSA provenance

4. Check /releases page shows v3.0.0-stage3 as "Latest release"

## Impact
- Resolves immediate release pipeline failure
- Enables future releases without requiring delete/re-push
- Fixes "silent failure" where release failures went unnoticed for 3 weeks

## Next Steps
After successful release, proceed with GITHUB-P24 to fix remaining release.yml fragilities.