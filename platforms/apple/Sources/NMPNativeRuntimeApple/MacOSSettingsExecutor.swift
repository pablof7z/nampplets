#if os(macOS)
import AppKit
import Foundation
import NMPNativeRuntime

// MARK: - AppKit settings presentation lifecycle

/// Finite AppKit settings executor. Rust supplies validated schema and current
/// values; this object renders controls and returns raw edits to Rust.
final class MacOSSettingsExecutor: NativeSettingsExecutor, @unchecked Sendable {
    private static let maximumWindows = 8

    private let lock = NSLock()
    private weak var controller: RuntimeController?
    private var pendingPresentations = 0
    private var isClosed = false
    private var windows: [String: NativeSettingsWindowController] = [:]

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
        guard windows.count + pendingPresentations < Self.maximumWindows else {
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
            let stale = windows.filter {
                !sessionIDs.contains($0.value.sessionID)
            }.map(\.value)
            lock.unlock()
            for window in stale {
                window.close()
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
        let active = Array(windows.values)
        windows.removeAll()
        lock.unlock()
        DispatchQueue.main.async {
            for window in active {
                window.close()
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
        if let existing = windows[key] {
            lock.unlock()
            existing.showWindow(nil)
            existing.window?.makeKeyAndOrderFront(nil)
            return
        }
        let window = NativeSettingsWindowController(
            document: document,
            onCommit: { [weak self] values, completion in
                self?.commit(document.request, values: values, completion: completion)
            },
            onClose: { [weak self] in
                self?.removeWindow(key)
            }
        )
        windows[key] = window
        lock.unlock()
        window.showWindow(nil)
        window.window?.makeKeyAndOrderFront(nil)
        NSApplication.shared.activate(ignoringOtherApps: true)
    }

    private func removeWindow(_ key: String) {
        lock.lock()
        windows.removeValue(forKey: key)
        lock.unlock()
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

final class NativeSettingsWindowController: NSWindowController, NSWindowDelegate {
    let sessionID: UInt64
    private let onClose: @Sendable () -> Void

    init(
        document: NativeSettingsDocument,
        onCommit: @escaping @Sendable ([String: Any], @escaping @Sendable (String?) -> Void) -> Void,
        onClose: @escaping @Sendable () -> Void
    ) {
        sessionID = document.request.sessionId
        self.onClose = onClose
        let content = NativeSettingsViewController(document: document, onCommit: onCommit)
        let window = NSWindow(contentViewController: content)
        window.title = "\(document.request.dTag) Settings"
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.setContentSize(NSSize(width: 560, height: 520))
        window.minSize = NSSize(width: 440, height: 360)
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        nil
    }

    func windowWillClose(_ notification: Notification) {
        onClose()
    }
}
#endif
