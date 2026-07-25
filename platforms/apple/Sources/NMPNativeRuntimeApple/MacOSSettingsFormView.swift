#if os(macOS)
import AppKit
import Foundation

// MARK: - AppKit settings form view controller

final class NativeSettingsViewController: NSViewController {
    enum FieldKind {
        case string
        case integer
        case number
        case boolean
        case array
        case enumeration([Any])
    }

    final class FieldBinding {
        let path: [String]
        let kind: FieldKind
        let control: NSControl
        let initiallyPresent: Bool
        var changed = false

        init(path: [String], kind: FieldKind, control: NSControl, initiallyPresent: Bool) {
            self.path = path
            self.kind = kind
            self.control = control
            self.initiallyPresent = initiallyPresent
        }
    }

    private let document: NativeSettingsDocument
    private let onCommit:
        @Sendable ([String: Any], @escaping @Sendable (String?) -> Void) -> Void
    var bindings: [FieldBinding] = []
    private var values: [String: Any]
    private let errorLabel = NSTextField(labelWithString: "")
    private let saveButton = NSButton(title: "Save", target: nil, action: nil)

    init(
        document: NativeSettingsDocument,
        onCommit: @escaping @Sendable ([String: Any], @escaping @Sendable (String?) -> Void) -> Void
    ) {
        self.document = document
        self.onCommit = onCommit
        values = document.values
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    override func loadView() {
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 14
        stack.edgeInsets = NSEdgeInsets(top: 20, left: 20, bottom: 20, right: 20)

        let title = NSTextField(labelWithString: titleText(document.schema, fallback: "Settings"))
        title.font = .preferredFont(forTextStyle: .title2)
        title.setAccessibilityRole(.staticText)
        stack.addArrangedSubview(title)
        if let description = descriptionText(document.schema) {
            let label = wrappingLabel(description)
            stack.addArrangedSubview(label)
        }
        addObject(
            schema: document.schema,
            current: document.values,
            path: [],
            to: stack,
            requestedSection: document.request.section
        )

        errorLabel.textColor = .systemRed
        errorLabel.maximumNumberOfLines = 3
        errorLabel.lineBreakMode = .byWordWrapping
        errorLabel.isHidden = true
        stack.addArrangedSubview(errorLabel)

        let buttons = NSStackView()
        buttons.orientation = .horizontal
        buttons.spacing = 8
        let cancel = NSButton(title: "Cancel", target: self, action: #selector(cancel))
        cancel.keyEquivalent = "\u{1b}"
        saveButton.target = self
        saveButton.action = #selector(save)
        saveButton.keyEquivalent = "\r"
        buttons.addArrangedSubview(cancel)
        buttons.addArrangedSubview(saveButton)
        stack.addArrangedSubview(buttons)

        let documentView = NSView()
        documentView.translatesAutoresizingMaskIntoConstraints = false
        stack.translatesAutoresizingMaskIntoConstraints = false
        documentView.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: documentView.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: documentView.trailingAnchor),
            stack.topAnchor.constraint(equalTo: documentView.topAnchor),
            stack.bottomAnchor.constraint(equalTo: documentView.bottomAnchor),
            stack.widthAnchor.constraint(greaterThanOrEqualToConstant: 400),
        ])
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.documentView = documentView
        view = scroll
    }

    @objc func fieldChanged(_ sender: NSControl) {
        bindings.first(where: { $0.control === sender })?.changed = true
    }

    @objc private func cancel() {
        view.window?.close()
    }

    @objc private func save() {
        do {
            var next = values
            for binding in bindings where binding.initiallyPresent || binding.changed {
                let value = try read(binding)
                set(value, at: binding.path, in: &next)
            }
            saveButton.isEnabled = false
            errorLabel.isHidden = true
            onCommit(next) { [weak self] error in
                DispatchQueue.main.async {
                    guard let self else { return }
                    if let error {
                        self.errorLabel.stringValue = error
                        self.errorLabel.isHidden = false
                        self.saveButton.isEnabled = true
                    } else {
                        self.values.removeAll(keepingCapacity: false)
                        self.view.window?.close()
                    }
                }
            }
        } catch {
            errorLabel.stringValue = error.localizedDescription
            errorLabel.isHidden = false
        }
    }
}
#endif
