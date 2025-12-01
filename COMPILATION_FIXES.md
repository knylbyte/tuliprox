# Compilation Fixes Required

## Identified Issues

### 1. TraktContentType Missing Traits
**File:** `shared/src/model/config/trakt.rs`
**Problem:** TraktContentType enum lacks Display and FromStr traits needed for dropdown functionality
**Fix:** Add Display and FromStr implementations

### 2. HashMap in generate_form_reducer
**File:** `frontend/src/app/components/source_editor/input_form.rs`
**Problem:** The `generate_form_reducer!` macro doesn't support HashMap<String, String> type
**Line:** 123 - `Headers => headers: HashMap<String, String>,`
**Fix:** Remove Headers from the reducer, handle it separately with use_state

### 3. Missing edit_field_number_u8 macro export
**File:** Should be in macros or needs to be added
**Problem:** `edit_field_number_u8` is used but may not be exported
**Fix:** Verify macro is properly exported or use edit_field_number instead

## Fixes to Apply

### Fix 1: Add TraktContentType Traits
Add to `shared/src/model/config/trakt.rs`:

```rust
use std::fmt;
use std::str::FromStr;

impl fmt::Display for TraktContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            TraktContentType::Vod => "Vod",
            TraktContentType::Series => "Series",
            TraktContentType::Both => "Both",
        })
    }
}

impl FromStr for TraktContentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "vod" => Ok(TraktContentType::Vod),
            "series" => Ok(TraktContentType::Series),
            "both" => Ok(TraktContentType::Both),
            _ => Err(format!("Invalid TraktContentType: {}", s)),
        }
    }
}
```

### Fix 2: Remove Headers from generate_form_reducer
In `input_form.rs`, change line 110-126 to NOT include Headers in the macro.
Handle headers separately with use_state (which is already done).

### Fix 3: Verify all state variables are declared
Check that all use_state declarations exist for:
- epg_sources_state ✓ (line 169)
- aliases_state ✓ (line 170)
- trakt_lists_state in output_xtream_form.rs

## Status
- [ ] TraktContentType traits added
- [ ] Headers removed from reducer
- [ ] All fixes tested
