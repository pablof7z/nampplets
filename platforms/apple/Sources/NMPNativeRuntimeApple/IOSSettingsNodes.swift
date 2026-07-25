#if os(iOS)
import Foundation
import SwiftUI

// MARK: - Settings schema node model and form state

struct SettingsField: Identifiable {
    enum Kind {
        case string(secret: Bool)
        case integer
        case number
        case boolean
        case array
        case enumeration(choices: [Any], labels: [String])
    }

    let id = UUID()
    let path: [String]
    let key: String
    let label: String
    let description: String?
    let kind: Kind
    let initiallyPresent: Bool
}

final class SettingsNode: Identifiable {
    enum Body {
        case field(SettingsField)
        case group(title: String, description: String?, children: [SettingsNode])
    }

    let id = UUID()
    let body: Body

    init(_ body: Body) {
        self.body = body
    }
}

func buildNodes(
    schema: [String: Any],
    current: [String: Any],
    path: [String],
    requestedSection: String?
) -> [SettingsNode] {
    let properties = schema["properties"] as? [String: Any] ?? [:]
    let ordered = properties.compactMap { key, value -> (String, [String: Any])? in
        guard let value = value as? [String: Any] else { return nil }
        return (key, value)
    }.sorted {
        let left = ($0.1["x-napplet-order"] as? NSNumber)?.doubleValue ?? .greatestFiniteMagnitude
        let right = ($1.1["x-napplet-order"] as? NSNumber)?.doubleValue ?? .greatestFiniteMagnitude
        return left == right ? $0.0 < $1.0 : left < right
    }
    var nodes: [SettingsNode] = []
    for (key, fieldSchema) in ordered {
        guard matchesSection(fieldSchema, requested: requestedSection) else { continue }
        let fieldPath = path + [key]
        let fieldValue = current[key]
        if fieldSchema["type"] as? String == "object" {
            let children = buildNodes(
                schema: fieldSchema,
                current: fieldValue as? [String: Any] ?? [:],
                path: fieldPath,
                requestedSection: requestedSection
            )
            nodes.append(SettingsNode(.group(
                title: titleText(fieldSchema, fallback: key),
                description: descriptionText(fieldSchema),
                children: children
            )))
            continue
        }
        nodes.append(SettingsNode(.field(buildField(
            key: key,
            schema: fieldSchema,
            value: fieldValue,
            path: fieldPath
        ))))
    }
    return nodes
}

private func buildField(
    key: String,
    schema: [String: Any],
    value: Any?,
    path: [String]
) -> SettingsField {
    let kind: SettingsField.Kind
    if let choices = schema["enum"] as? [Any], !choices.isEmpty {
        let descriptions = schema["enumDescriptions"] as? [String]
        let labels = choices.enumerated().map { index, choice in
            descriptions?[safe: index] ?? String(describing: choice)
        }
        kind = .enumeration(choices: choices, labels: labels)
    } else {
        switch schema["type"] as? String {
        case "boolean": kind = .boolean
        case "integer": kind = .integer
        case "number": kind = .number
        case "array": kind = .array
        default: kind = .string(secret: schema["x-napplet-secret"] as? Bool == true)
        }
    }
    return SettingsField(
        path: path,
        key: path.joined(separator: "."),
        label: titleText(schema, fallback: key),
        description: descriptionText(schema),
        kind: kind,
        initiallyPresent: value != nil
    )
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

@MainActor
final class SettingsFormStore: ObservableObject {
    @Published var stringValues: [String: String] = [:]
    @Published var boolValues: [String: Bool] = [:]
    @Published var enumIndices: [String: Int] = [:]
    @Published var changedKeys: Set<String> = []
    @Published var errorMessage: String?
    @Published var isSaving = false

    init(nodes: [SettingsNode], values: [String: Any]) {
        seed(nodes: nodes, values: values)
    }

    private func seed(nodes: [SettingsNode], values: [String: Any]) {
        for node in nodes {
            switch node.body {
            case let .field(field):
                let value = valueAt(field.path, in: values)
                switch field.kind {
                case .boolean:
                    boolValues[field.key] = (value as? Bool) == true
                case let .enumeration(choices, _):
                    if let value, let index = choices.firstIndex(where: { jsonEqual($0, value) }) {
                        enumIndices[field.key] = index
                    }
                case .array:
                    stringValues[field.key] = jsonString(value) ?? "[]"
                default:
                    stringValues[field.key] = value.map { String(describing: $0) } ?? ""
                }
            case let .group(_, _, children):
                seed(nodes: children, values: values)
            }
        }
    }

    private func valueAt(_ path: [String], in values: [String: Any]) -> Any? {
        guard let head = path.first else { return nil }
        if path.count == 1 { return values[head] }
        guard let nested = values[head] as? [String: Any] else { return nil }
        return valueAt(Array(path.dropFirst()), in: nested)
    }

    func read(_ field: SettingsField) throws -> Any {
        switch field.kind {
        case .string:
            return stringValues[field.key] ?? ""
        case .integer:
            guard let value = Int64(stringValues[field.key] ?? "") else {
                throw SettingsPresentationError.invalidInteger
            }
            return value
        case .number:
            guard let value = Double(stringValues[field.key] ?? ""), value.isFinite else {
                throw SettingsPresentationError.invalidNumber
            }
            return value
        case .boolean:
            return boolValues[field.key] ?? false
        case .array:
            let text = stringValues[field.key] ?? "[]"
            guard let data = text.data(using: .utf8),
                  let array = try? JSONSerialization.jsonObject(with: data) as? [Any]
            else {
                throw SettingsPresentationError.invalidArray
            }
            return array
        case let .enumeration(choices, _):
            guard let index = enumIndices[field.key], choices.indices.contains(index) else {
                throw SettingsPresentationError.invalidChoice
            }
            return choices[index]
        }
    }
}

func flatten(_ nodes: [SettingsNode]) -> [SettingsField] {
    nodes.flatMap { node -> [SettingsField] in
        switch node.body {
        case let .field(field): [field]
        case let .group(_, _, children): flatten(children)
        }
    }
}
#endif
