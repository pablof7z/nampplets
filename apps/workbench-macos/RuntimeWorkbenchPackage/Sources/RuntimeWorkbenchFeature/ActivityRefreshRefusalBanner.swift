import SwiftUI

struct ActivityRefreshRefusalBanner: View {
    let refusal: RuntimeWorkbenchActivitySourceRefusal

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Activity couldn’t refresh")
                .font(.headline)
            Text("Showing the last accepted activity; it may be out of date.")
                .font(.caption)
                .foregroundStyle(.secondary)
            DisclosureGroup("Technical details") {
                Text(refusal.localizedDescription)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            .font(.caption)
        }
        .padding()
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
