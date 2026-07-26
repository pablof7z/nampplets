import SwiftUI

/// Shown when the runtime refused a refresh and the drawer is therefore
/// displaying the last snapshot it accepted.
///
/// This arrived from #142 already shaped the way ADR 0008 asks for -- a plain
/// verdict with the runtime's own words behind a disclosure -- which is a
/// pleasant convergence rather than something I had to impose. All that
/// changed is that it now uses the shared primitives, so "Technical details"
/// is the same affordance here as everywhere else rather than a second
/// disclosure that merely looks similar.
struct ActivityRefreshRefusalBanner: View {
    let refusal: RuntimeWorkbenchActivitySourceRefusal

    var body: some View {
        VStack(alignment: .leading, spacing: NappletMetrics.hairline) {
            Text("Activity couldn’t refresh")
                .font(NappletType.heading)
                .foregroundStyle(NappletInk.ink)
            Text("Showing the last accepted activity; it may be out of date.")
                .font(NappletType.caption)
                .foregroundStyle(NappletInk.inkSecondary)
                .fixedSize(horizontal: false, vertical: true)

            NappletEvidence {
                NappletFieldGrid(fields: [
                    NappletField("Refusal", refusal.localizedDescription),
                ])
            }
            .font(NappletType.caption)
        }
        .padding(NappletMetrics.comfortable)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
    }
}
