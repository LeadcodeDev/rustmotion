//! Merging rustmotion's guidance into a project `CLAUDE.md` without owning the file.
//!
//! `skills install` used to write `CLAUDE.md` wholesale and `skills uninstall` used
//! to delete it. Both treated a file the user authors as rustmotion's property: a
//! project with its own build notes lost them on install, and lost the file itself
//! on uninstall.
//!
//! Instead, rustmotion claims a delimited block and never touches anything outside
//! it. The markers are HTML comments so they stay invisible in rendered Markdown.

pub const START: &str = "<!-- rustmotion:start -->";
pub const END: &str = "<!-- rustmotion:end -->";

/// Byte range of the rustmotion block, markers included.
fn block_span(text: &str) -> Option<(usize, usize)> {
    let start = text.find(START)?;
    let end = text[start..].find(END)? + start + END.len();
    Some((start, end))
}

/// The document to write on install.
///
/// - no file yet: the block alone;
/// - a file without a block: the block appended, existing content untouched;
/// - a file with a block: only the block replaced, in place.
pub fn merge(existing: Option<&str>, body: &str) -> String {
    let block = format!("{START}\n{}\n{END}\n", body.trim_end());
    let Some(existing) = existing else {
        return block;
    };
    match block_span(existing) {
        Some((start, end)) => {
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..start]);
            out.push_str(block.trim_end());
            out.push_str(&existing[end..]);
            out
        }
        None if existing.trim().is_empty() => block,
        None => {
            let mut out = existing.to_string();
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
            out.push_str(&block);
            out
        }
    }
}

/// The document to write on uninstall, or `None` when nothing rustmotion owns is
/// left and the file should be removed.
///
/// A file the user also wrote in survives with its own content; a file that only
/// ever held our block is reported as removable.
pub fn strip(existing: &str) -> Option<String> {
    let Some((start, end)) = block_span(existing) else {
        // No block: the file is entirely the user's. Never remove it.
        return Some(existing.to_string());
    };
    let mut out = String::with_capacity(existing.len());
    out.push_str(&existing[..start]);
    out.push_str(&existing[end..]);
    if out.trim().is_empty() {
        None
    } else {
        Some(format!("{}\n", out.trim_end()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "# rustmotion\nGuidance.";

    #[test]
    fn a_project_claude_md_survives_install_and_uninstall() {
        let user = "# My Project\nBuild with `make`. Never delete this.\n";

        let installed = merge(Some(user), BODY);
        assert!(
            installed.contains("Never delete this."),
            "install destroyed the user's content: {installed}"
        );
        assert!(installed.contains(BODY));

        let uninstalled = strip(&installed).expect("a user-authored file is never removed");
        assert!(uninstalled.contains("Never delete this."));
        assert!(
            !uninstalled.contains("Guidance."),
            "uninstall left our block behind: {uninstalled}"
        );
        assert!(!uninstalled.contains(START));
    }

    #[test]
    fn reinstalling_replaces_the_block_instead_of_stacking_copies() {
        let once = merge(None, BODY);
        let twice = merge(Some(&once), "# rustmotion\nNewer guidance.");
        assert_eq!(twice.matches(START).count(), 1, "block duplicated: {twice}");
        assert!(twice.contains("Newer guidance."));
        assert!(!twice.contains("Guidance."));
    }

    #[test]
    fn a_file_we_alone_created_is_reported_as_removable() {
        let ours = merge(None, BODY);
        assert!(strip(&ours).is_none());
    }

    #[test]
    fn a_file_without_our_block_is_returned_untouched() {
        let user = "# Theirs\nnothing of ours here\n";
        assert_eq!(strip(user).as_deref(), Some(user));
    }

    #[test]
    fn content_around_the_block_is_preserved_on_both_sides() {
        let doc = format!("before\n\n{START}\nold\n{END}\n\nafter\n");
        let merged = merge(Some(&doc), BODY);
        assert!(merged.starts_with("before"));
        assert!(merged.trim_end().ends_with("after"));
        assert!(merged.contains("Guidance."));
        assert!(!merged.contains("\nold\n"));
    }
}
