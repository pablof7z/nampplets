import SwiftUI

struct ActivityRefreshRefusalBanner: View {
    let refusal: RuntimeWorkbenchActivitySourceRefusal

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "exclamationmark.triangle")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text("Activity refresh refused")
                    .font(.headline)
                Text(
                    "The last accepted activity remains visible and was not refreshed."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
                Text(refusal.localizedDescription)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            Spacer()
        }
        .padding()
        .background(.orange.opacity(0.08))
        .accessibilityElement(children: .combine)
    }
}
