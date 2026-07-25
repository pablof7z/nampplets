#if os(iOS)
import Foundation
import SwiftUI

// MARK: - SwiftUI settings form presentation

private struct SettingsFieldRow: View {
    let field: SettingsField
    @ObservedObject var store: SettingsFormStore

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            control
            if let description = field.description {
                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(field.label)
    }

    @ViewBuilder
    private var control: some View {
        switch field.kind {
        case let .string(secret):
            if secret {
                SecureField(field.label, text: binding(for: field.key))
            } else {
                TextField(field.label, text: binding(for: field.key))
            }
        case .integer, .number:
            TextField(field.label, text: binding(for: field.key))
                .keyboardType(.numbersAndPunctuation)
        case .array:
            TextField(field.label, text: binding(for: field.key))
                .autocapitalization(.none)
        case .boolean:
            Toggle(field.label, isOn: boolBinding(for: field.key))
        case let .enumeration(_, labels):
            Picker(field.label, selection: enumBinding(for: field.key)) {
                ForEach(Array(labels.enumerated()), id: \.offset) { index, label in
                    Text(label).tag(index)
                }
            }
        }
    }

    private func binding(for key: String) -> Binding<String> {
        Binding(
            get: { store.stringValues[key] ?? "" },
            set: {
                store.stringValues[key] = $0
                store.changedKeys.insert(key)
            }
        )
    }

    private func boolBinding(for key: String) -> Binding<Bool> {
        Binding(
            get: { store.boolValues[key] ?? false },
            set: {
                store.boolValues[key] = $0
                store.changedKeys.insert(key)
            }
        )
    }

    private func enumBinding(for key: String) -> Binding<Int> {
        Binding(
            get: { store.enumIndices[key] ?? 0 },
            set: {
                store.enumIndices[key] = $0
                store.changedKeys.insert(key)
            }
        )
    }
}

private struct SettingsNodeView: View {
    let node: SettingsNode
    @ObservedObject var store: SettingsFormStore

    var body: some View {
        switch node.body {
        case let .field(field):
            SettingsFieldRow(field: field, store: store)
        case let .group(title, description, children):
            GroupBox(title) {
                VStack(alignment: .leading, spacing: 12) {
                    if let description {
                        Text(description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    ForEach(children) { child in
                        SettingsNodeView(node: child, store: store)
                    }
                }
            }
        }
    }
}

struct SettingsFormView: View {
    let document: NativeSettingsDocument
    let nodes: [SettingsNode]
    let fields: [SettingsField]
    @ObservedObject var store: SettingsFormStore
    let onCommit: @Sendable ([String: Any], @escaping @Sendable (String?) -> Void) -> Void
    let onDismiss: () -> Void

    var body: some View {
        NavigationStack {
            Form {
                if let description = descriptionText(document.schema) {
                    Text(description)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                ForEach(nodes) { node in
                    SettingsNodeView(node: node, store: store)
                }
                if let errorMessage = store.errorMessage {
                    Text(errorMessage)
                        .font(.footnote)
                        .foregroundStyle(.red)
                }
            }
            .navigationTitle(titleText(document.schema, fallback: "\(document.request.dTag) Settings"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: onDismiss)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save", action: save)
                        .disabled(store.isSaving)
                }
            }
        }
    }

    private func save() {
        var next = document.values
        do {
            for field in fields where field.initiallyPresent || store.changedKeys.contains(field.key) {
                let value = try store.read(field)
                set(value, at: field.path, in: &next)
            }
        } catch {
            store.errorMessage = error.localizedDescription
            return
        }
        store.isSaving = true
        store.errorMessage = nil
        onCommit(next) { error in
            DispatchQueue.main.async {
                store.isSaving = false
                if let error {
                    store.errorMessage = error
                } else {
                    onDismiss()
                }
            }
        }
    }

    private func set(_ value: Any, at path: [String], in object: inout [String: Any]) {
        guard let head = path.first else { return }
        if path.count == 1 {
            object[head] = value
            return
        }
        var nested = object[head] as? [String: Any] ?? [:]
        set(value, at: Array(path.dropFirst()), in: &nested)
        object[head] = nested
    }
}
#endif
