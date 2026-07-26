import SwiftUI

/// How a catalog failure reaches a person.
///
/// `CatalogIssue` is constructed at eighteen sites, most of which project a
/// runtime refusal verbatim -- "the application runtime profile is
/// unavailable", "no more than 2048 UTF-8 bytes", "manual manifest
/// coordinate". Rendering those directly put the diagnostic console back on
/// the verdict path at exactly the moment a person is least equipped to read
/// it: something has just gone wrong.
///
/// Fixing eighteen call sites would be the wrong repair. Most of those strings
/// are Rust's and correct as evidence; what was wrong was rendering evidence
/// as a verdict. So the correction is here, at the boundary where an issue
/// becomes something a person sees: a calm sentence the shell stands behind,
/// chosen by what the person was trying to do, and the runtime's own words
/// kept intact one deliberate move away.
///
/// See `docs/adr/0008-verdicts-on-the-path.md`. Caught by @opal-codex's
/// whole-surface audit.
struct CatalogIssueNotice: View {
    enum Context {
        /// Browsing or searching the catalog.
        case browse
        /// Resolving something before showing what it is.
        case resolve
        /// Acquiring the verified bytes.
        case install

        var sentence: String {
            switch self {
            case .browse:
                "Couldn't load napplets just now."
            case .resolve:
                "Couldn't open that napplet."
            case .install:
                "Couldn't add that napplet."
            }
        }
    }

    let issue: CatalogIssue
    let context: Context

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.tight) {
            NappletNotice(verdict: .caution(context.sentence))

            NappletEvidence(label: "What the runtime said") {
                NappletFieldGrid(fields: [
                    NappletField(issue.title, issue.message),
                ])
            }
            .font(NappletType.caption)
        }
        // Deliberately unidentified, for the third time in this branch and
        // hopefully the last. SwiftUI propagates an `accessibilityIdentifier`
        // across the subtree it covers, so one here would mask the evidence
        // disclosure and the raw fields underneath it -- the same defect that
        // took two CI runs to pin down on `ContentUnavailableView` and
        // `ActivityDrawer`. Nothing queries this container. If something ever
        // needs to, identify the leaf it actually cares about.
    }
}
