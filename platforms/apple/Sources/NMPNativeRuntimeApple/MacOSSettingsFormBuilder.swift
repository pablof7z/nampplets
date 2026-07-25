#if os(macOS)
import AppKit
import Foundation

// MARK: - AppKit settings form construction and readback

extension NativeSettingsViewController {
    func addObject(
        schema: [String: Any],
        current: [String: Any],
        path: [String],
        to stack: NSStackView,
        requestedSection: String?
    ) {
        let properties = schema["properties"] as? [String: Any] ?? [:]
        let ordered = properties.compactMap { key, value -> (String, [String: Any])? in
            guard let value = value as? [String: Any] else { return nil }
            return (key, value)
        }.sorted {
            let left = ($0.1["x-napplet-order"] as? NSNumber)?.doubleValue ?? .greatestFiniteMagnitude
            let right = ($1.1["x-napplet-order"] as? NSNumber)?.doubleValue ?? .greatestFiniteMagnitude
            return left == right ? $0.0 < $1.0 : left < right
        }
        for (key, fieldSchema) in ordered {
            guard matchesSection(fieldSchema, requested: requestedSection) else { continue }
            let fieldPath = path + [key]
            let fieldValue = current[key]
            if fieldSchema["type"] as? String == "object" {
                let nested = NSStackView()
                nested.orientation = .vertical
                nested.alignment = .leading
                nested.spacing = 10
                let heading = NSTextField(
                    labelWithString: titleText(fieldSchema, fallback: key)
                )
                heading.font = .preferredFont(forTextStyle: .headline)
                heading.setAccessibilityRole(.staticText)
                nested.addArrangedSubview(heading)
                if let description = descriptionText(fieldSchema) {
                    nested.addArrangedSubview(wrappingLabel(description))
                }
                addObject(
                    schema: fieldSchema,
                    current: fieldValue as? [String: Any] ?? [:],
                    path: fieldPath,
                    to: nested,
                    requestedSection: requestedSection
                )
                let box = NSBox()
                box.boxType = .primary
                box.contentView = nested
                stack.addArrangedSubview(box)
                box.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
                continue
            }
            addField(
                key: key,
                schema: fieldSchema,
                value: fieldValue,
                path: fieldPath,
                to: stack
            )
        }
    }

    private func addField(
        key: String,
        schema: [String: Any],
        value: Any?,
        path: [String],
        to stack: NSStackView
    ) {
        let row = NSStackView()
        row.orientation = .horizontal
        row.alignment = .firstBaseline
        row.spacing = 12
        let label = NSTextField(labelWithString: titleText(schema, fallback: key))
        label.alignment = .right
        label.widthAnchor.constraint(equalToConstant: 150).isActive = true
        row.addArrangedSubview(label)

        let control: NSControl
        let kind: FieldKind
        if let choices = schema["enum"] as? [Any], !choices.isEmpty {
            let popup = NSPopUpButton()
            let descriptions = schema["enumDescriptions"] as? [String]
            popup.addItems(withTitles: choices.enumerated().map { index, choice in
                descriptions?[safe: index] ?? String(describing: choice)
            })
            if let value,
               let selected = choices.firstIndex(where: { jsonEqual($0, value) }) {
                popup.selectItem(at: selected)
            }
            control = popup
            kind = .enumeration(choices)
        } else {
            switch schema["type"] as? String {
            case "boolean":
                let toggle = NSSwitch()
                toggle.state = (value as? Bool) == true ? .on : .off
                control = toggle
                kind = .boolean
            case "integer":
                let input = NSTextField(string: value.map(String.init(describing:)) ?? "")
                input.alignment = .right
                control = input
                kind = .integer
            case "number":
                let input = NSTextField(string: value.map(String.init(describing:)) ?? "")
                input.alignment = .right
                control = input
                kind = .number
            case "array":
                let input = NSTextField(string: jsonString(value) ?? "[]")
                input.placeholderString = "JSON array"
                control = input
                kind = .array
            default:
                let secret = schema["x-napplet-secret"] as? Bool == true
                let input: NSTextField = secret
                    ? NSSecureTextField(string: value as? String ?? "")
                    : NSTextField(string: value as? String ?? "")
                control = input
                kind = .string
            }
        }
        control.target = self
        control.action = #selector(fieldChanged(_:))
        control.setAccessibilityLabel(label.stringValue)
        if let description = descriptionText(schema) {
            control.setAccessibilityHelp(description)
        }
        control.widthAnchor.constraint(greaterThanOrEqualToConstant: 220).isActive = true
        row.addArrangedSubview(control)
        stack.addArrangedSubview(row)
        row.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        if let description = descriptionText(schema) {
            let help = wrappingLabel(description)
            help.textColor = .secondaryLabelColor
            help.font = .preferredFont(forTextStyle: .caption1)
            stack.addArrangedSubview(help)
        }
        bindings.append(
            FieldBinding(
                path: path,
                kind: kind,
                control: control,
                initiallyPresent: value != nil
            )
        )
    }

    func read(_ binding: FieldBinding) throws -> Any {
        switch binding.kind {
        case .string:
            return (binding.control as? NSTextField)?.stringValue ?? ""
        case .integer:
            guard let value = Int64((binding.control as? NSTextField)?.stringValue ?? "") else {
                throw SettingsPresentationError.invalidInteger
            }
            return value
        case .number:
            guard let value = Double((binding.control as? NSTextField)?.stringValue ?? ""),
                  value.isFinite
            else {
                throw SettingsPresentationError.invalidNumber
            }
            return value
        case .boolean:
            return (binding.control as? NSSwitch)?.state == .on
        case .array:
            let text = (binding.control as? NSTextField)?.stringValue ?? "[]"
            guard let data = text.data(using: .utf8),
                  let array = try? JSONSerialization.jsonObject(with: data) as? [Any]
            else {
                throw SettingsPresentationError.invalidArray
            }
            return array
        case let .enumeration(choices):
            let index = (binding.control as? NSPopUpButton)?.indexOfSelectedItem ?? -1
            guard choices.indices.contains(index) else {
                throw SettingsPresentationError.invalidChoice
            }
            return choices[index]
        }
    }

    func set(_ value: Any, at path: [String], in object: inout [String: Any]) {
        guard let head = path.first else { return }
        if path.count == 1 {
            object[head] = value
            return
        }
        var nested = object[head] as? [String: Any] ?? [:]
        set(value, at: Array(path.dropFirst()), in: &nested)
        object[head] = nested
    }

    private func matchesSection(_ schema: [String: Any], requested: String?) -> Bool {
        guard let requested else { return true }
        if schema["x-napplet-section"] as? String == requested {
            return true
        }
        guard schema["type"] as? String == "object",
              let properties = schema["properties"] as? [String: Any]
        else {
            return false
        }
        return properties.values.contains {
            ($0 as? [String: Any]).isSomeAnd { matchesSection($0, requested: requested) }
        }
    }
}

@MainActor
func wrappingLabel(_ value: String) -> NSTextField {
    let label = NSTextField(wrappingLabelWithString: value)
    label.maximumNumberOfLines = 0
    return label
}
#endif
