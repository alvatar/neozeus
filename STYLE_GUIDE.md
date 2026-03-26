# STYLE_GUIDE

## Modules
- Root = one role: namespace, curated API, or impl; not mixed.
- Prefer small curated root exports; avoid barrel/facade roots.
- Prefer leaf imports over root re-export churn.
- `pub(crate)` > `pub`; expose only true subsystem API.
- Avoid `pub(crate) mod`; use only when path traversal is intended.
- Test-only exports live in one `#[cfg(test)]` block, after prod exports.
- Delete temporary compatibility facades when callers are migrated.
- Flatten singleton `mod.rs` roots when no subtree clarity is gained.

## Tests
- Prefer separate sibling test submodules: `foo.rs` + `foo/tests.rs`.
- Keep tests near the owning module.
- Leave inline tests only if tiny and private-helper-specific.
- Move large inline test blocks out of impl files.
- Keep cross-module/system/ECS behavior tests centralized.
- Do not widen prod visibility just to satisfy tests.
- Module-local behavior tests belong with the module.
- Parser/serializer/helper tests should not live in giant central buckets.
