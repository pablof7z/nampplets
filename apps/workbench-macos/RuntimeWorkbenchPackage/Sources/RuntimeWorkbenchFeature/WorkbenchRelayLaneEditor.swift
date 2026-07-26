import SwiftUI

struct WorkbenchRelayLaneEditor: View {
    let title: String
    let detail: String
    let identifierPrefix: String
    @Binding var relays: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.headline)
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            ForEach(relays.indices, id: \.self) { index in
                HStack(spacing: 8) {
                    TextField(
                        "wss://relay.example",
                        text: Binding(
                            get: { relays[index] },
                            set: { relays[index] = $0 }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .autocorrectionDisabled()
                    .accessibilityLabel("\(title) address \(index + 1)")
                    .accessibilityIdentifier(
                        "\(identifierPrefix)-relay-\(index)"
                    )

                    Button {
                        relays.remove(at: index)
                    } label: {
                        Image(systemName: "minus.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Remove \(title) address")
                }
            }

            Button {
                relays.append("")
            } label: {
                Label("Add address", systemImage: "plus")
            }
            .buttonStyle(.plain)
            .disabled(
                relays.count
                    >= WorkbenchProfilePreferences.maximumRelaysPerGroup
            )
            .accessibilityIdentifier("\(identifierPrefix)-relay-add")
        }
        .padding(.vertical, 4)
    }
}
