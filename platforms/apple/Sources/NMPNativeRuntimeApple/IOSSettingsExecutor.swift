#if os(iOS)
import Foundation
import NMPNativeRuntime
import SwiftUI
import UIKit

// MARK: - UIKit settings presentation lifecycle

/// Finite UIKit settings executor. Rust supplies validated schema and current
/// values; this object presents a SwiftUI form and returns raw edits to Rust.
final class IOSSettingsExecutor: NativeSettingsExecutor, @unchecked Sendable {
    private static let maximumPresentations = 8

    private let lock = NSLock()
    private weak var controller: RuntimeController?
    private var pendingPresentations = 0
    private var isClosed = false
    private var presentations: [String: (sessionID: UInt64, controller: UIViewController)] = [:]

    func bind(controller: RuntimeController) {
        lock.lock()
        if !isClosed {
            self.controller = controller
        }
        lock.unlock()
    }

    func tryOpen(request: NativeSettingsRequest) -> NativeSettingsOpenResult {
        guard let document = NativeSettingsDocument.decode(request) else {
            return .unavailable
        }
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            return .closed
        }
        guard presentations.count + pendingPresentations < Self.maximumPresentations else {
            lock.unlock()
            return .saturated
        }
        pendingPresentations += 1
        lock.unlock()
        DispatchQueue.main.async { [weak self] in
            self?.present(document)
        }
        return .accepted
    }

    func retainRunningSessions(_ sessionIDs: Set<UInt64>) {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            lock.lock()
            let stale = presentations.filter { !sessionIDs.contains($0.value.sessionID) }
            lock.unlock()
            for (key, entry) in stale {
                dismiss(key: key, controller: entry.controller)
            }
        }
    }

    func close() {
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            return
        }
        isClosed = true
        controller = nil
        let active = presentations
        presentations.removeAll()
        lock.unlock()
        DispatchQueue.main.async {
            for (key, entry) in active {
                self.dismiss(key: key, controller: entry.controller)
            }
        }
    }

    @MainActor
    private func present(_ document: NativeSettingsDocument) {
        let key = Self.key(document.request)
        lock.lock()
        pendingPresentations = max(0, pendingPresentations - 1)
        guard !isClosed else {
            lock.unlock()
            return
        }
        guard presentations[key] == nil else {
            lock.unlock()
            return
        }
        guard let presenter = keyWindow()?.rootViewController else {
            lock.unlock()
            return
        }
        let nodes = buildNodes(
            schema: document.schema,
            current: document.values,
            path: [],
            requestedSection: document.request.section
        )
        let fields = flatten(nodes)
        let store = SettingsFormStore(nodes: nodes, values: document.values)
        let hosting = UIHostingController(
            rootView: SettingsFormView(
                document: document,
                nodes: nodes,
                fields: fields,
                store: store,
                onCommit: { [weak self] values, completion in
                    self?.commit(document.request, values: values, completion: completion)
                },
                onDismiss: { [weak self] in
                    self?.removePresentation(key)
                }
            )
        )
        hosting.modalPresentationStyle = .formSheet
        hosting.isModalInPresentation = true
        presentations[key] = (document.request.sessionId, hosting)
        lock.unlock()
        presenter.present(hosting, animated: true)
    }

    private func removePresentation(_ key: String) {
        lock.lock()
        let entry = presentations.removeValue(forKey: key)
        lock.unlock()
        if let entry {
            DispatchQueue.main.async {
                entry.controller.dismiss(animated: true)
            }
        }
    }

    @MainActor
    private func dismiss(key: String, controller: UIViewController) {
        lock.lock()
        presentations.removeValue(forKey: key)
        lock.unlock()
        controller.dismiss(animated: true)
    }

    private func commit(
        _ request: NativeSettingsRequest,
        values: [String: Any],
        completion: @escaping @Sendable (String?) -> Void
    ) {
        guard JSONSerialization.isValidJSONObject(values),
              let data = try? JSONSerialization.data(
                  withJSONObject: values,
                  options: [.sortedKeys]
              ),
              data.count <= 192 * 1_024,
              let valuesJSON = String(data: data, encoding: .utf8)
        else {
            completion("The edited settings could not be encoded.")
            return
        }
        lock.lock()
        let controller = isClosed ? nil : controller
        lock.unlock()
        guard let controller else {
            completion("The runtime settings capability is closed.")
            return
        }
        DispatchQueue.global(qos: .userInitiated).async {
            let update = controller.commitConfigValues(
                commit: NativeConfigCommit(
                    manifestAuthor: request.manifestAuthor,
                    dTag: request.dTag,
                    aggregateHash: request.aggregateHash,
                    sessionId: request.sessionId,
                    valuesJson: valuesJSON
                )
            )
            completion(update.accepted ? nil : update.refusal?.detail ?? "Settings were refused.")
        }
    }

    private static func key(_ request: NativeSettingsRequest) -> String {
        [
            request.manifestAuthor,
            request.dTag,
            request.aggregateHash,
            String(request.sessionId),
        ].joined(separator: ":")
    }
}
#endif
