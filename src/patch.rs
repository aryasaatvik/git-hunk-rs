use crate::cli::Mode;
use crate::error::{AppError, AppResult};
use crate::model::{ChangeState, ScanState};
use crate::select::ResolvedSelection;

pub fn build_patch(state: &ScanState, selection: &ResolvedSelection) -> AppResult<String> {
    let mut out = String::new();

    for (file_index, change_indexes) in &selection.per_file_change_indexes {
        let file = &state.files[*file_index];
        if change_indexes.is_empty() {
            continue;
        }

        if file.patch_header_lines.is_empty() {
            return Err(AppError::new(
                "invalid_patch",
                format!("file '{}' is missing patch headers", file.path),
            ));
        }

        for line in &file.patch_header_lines {
            out.push_str(line);
            out.push('\n');
        }
        // `git apply --unidiff-zero` places each hunk at its result-side line, and a pure
        // insertion has no preimage to correct that guess. The scanned headers count every
        // change, so shift each selected one past the changes left out of this patch.
        let mut skipped_delta: i64 = 0;
        for (change_index, change) in file.changes.iter().enumerate() {
            if !change_indexes.contains(&change_index) {
                skipped_delta += i64::from(change.new_lines) - i64::from(change.old_lines);
                continue;
            }
            out.push_str(&adjusted_header(
                change,
                state.snapshot.mode,
                skipped_delta,
            )?);
            out.push('\n');
            for line in &change.lines {
                out.push_str(&line.raw);
                out.push('\n');
            }
        }
    }

    if out.is_empty() {
        return Err(AppError::new(
            "empty_patch",
            "selection did not produce a patch".to_string(),
        ));
    }

    Ok(out)
}

fn adjusted_header(change: &ChangeState, mode: Mode, skipped_delta: i64) -> AppResult<String> {
    if skipped_delta == 0 {
        return Ok(change.header.clone());
    }

    let (old_start, new_start) = match mode {
        // The index holds the old side; the result drops the skipped changes.
        Mode::Stage => (
            i64::from(change.old_start),
            i64::from(change.new_start) - skipped_delta,
        ),
        // Reversed, the old side becomes the result, which keeps the skipped changes.
        Mode::Unstage => (
            i64::from(change.old_start) + skipped_delta,
            i64::from(change.new_start),
        ),
    };
    if old_start < 0 || new_start < 0 {
        return Err(AppError::new(
            "invalid_patch",
            format!("could not rebase hunk header '{}'", change.header),
        ));
    }

    let section = change
        .header
        .get(2..)
        .and_then(|rest| rest.find("@@"))
        .map(|end| &change.header[end + 4..])
        .unwrap_or("");
    Ok(format!(
        "@@ -{},{} +{},{} @@{}",
        old_start, change.old_lines, new_start, change.new_lines, section
    ))
}
