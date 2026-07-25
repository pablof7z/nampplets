#if os(macOS)
import AppKit
import Foundation
import NMPNativeRuntime

// MARK: - AppKit appearance projection

/// Event-driven projection of AppKit appearance facts. The callback reports
/// raw OS traits only; Rust maps them to the pinned NAP-THEME schema.
final class MacOSAppearanceSource: NSObject, NativeAppearanceSource, @unchecked Sendable {
    private let lock = NSLock()
    private var snapshot: NativeAppearanceSnapshot
    private weak var controller: RuntimeController?
    private var isClosed = false
    private var refreshPending = false
    private var appearanceObservation: NSKeyValueObservation?
    private var accessibilityObserver: NSObjectProtocol?
    private var colorObserver: NSObjectProtocol?

    override init() {
        snapshot = Self.captureSynchronously()
        super.init()
    }

    func current() -> NativeAppearanceSnapshot? {
        lock.lock()
        defer { lock.unlock() }
        return isClosed ? nil : snapshot
    }

    func bind(controller: RuntimeController) {
        lock.lock()
        guard !isClosed else {
            lock.unlock()
            return
        }
        self.controller = controller
        lock.unlock()
        DispatchQueue.main.async { [weak self] in
            self?.installObservers()
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
        lock.unlock()
        DispatchQueue.main.async { [weak self] in
            self?.removeObservers()
        }
    }

    @MainActor
    private func installObservers() {
        guard appearanceObservation == nil else { return }
        appearanceObservation = NSApplication.shared.observe(
            \.effectiveAppearance,
            options: [.new]
        ) { [weak self] _, _ in
            self?.scheduleRefresh()
        }
        accessibilityObserver = NotificationCenter.default.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: NSWorkspace.shared,
            queue: .main
        ) { [weak self] _ in
            self?.scheduleRefresh()
        }
        colorObserver = NotificationCenter.default.addObserver(
            forName: NSColor.systemColorsDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.scheduleRefresh()
        }
    }

    @MainActor
    private func removeObservers() {
        appearanceObservation?.invalidate()
        appearanceObservation = nil
        if let accessibilityObserver {
            NotificationCenter.default.removeObserver(accessibilityObserver)
            self.accessibilityObserver = nil
        }
        if let colorObserver {
            NotificationCenter.default.removeObserver(colorObserver)
            self.colorObserver = nil
        }
    }

    private func scheduleRefresh() {
        lock.lock()
        guard !isClosed, !refreshPending else {
            lock.unlock()
            return
        }
        refreshPending = true
        lock.unlock()
        DispatchQueue.main.async { [weak self] in
            self?.publishCurrentAppearance()
        }
    }

    @MainActor
    private func publishCurrentAppearance() {
        let next = Self.captureOnMainActor()
        lock.lock()
        refreshPending = false
        guard !isClosed else {
            lock.unlock()
            return
        }
        let changed = next != snapshot
        snapshot = next
        let controller = controller
        lock.unlock()
        if changed {
            _ = controller?.updateAppearance(appearance: next)
        }
    }

    private static func captureSynchronously() -> NativeAppearanceSnapshot {
        if Thread.isMainThread {
            return MainActor.assumeIsolated { captureOnMainActor() }
        }
        return DispatchQueue.main.sync {
            MainActor.assumeIsolated { captureOnMainActor() }
        }
    }

    @MainActor
    private static func captureOnMainActor() -> NativeAppearanceSnapshot {
        let appearance = NSApplication.shared.effectiveAppearance
        let dark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let workspace = NSWorkspace.shared
        let accent = NSColor.controlAccentColor.usingColorSpace(.sRGB) ?? .systemBlue
        return NativeAppearanceSnapshot(
            dark: dark,
            increasedContrast: workspace.accessibilityDisplayShouldIncreaseContrast,
            reducedTransparency: workspace.accessibilityDisplayShouldReduceTransparency,
            accentRed: component(accent.redComponent),
            accentGreen: component(accent.greenComponent),
            accentBlue: component(accent.blueComponent)
        )
    }

    private static func component(_ value: CGFloat) -> UInt8 {
        UInt8(clamping: Int((min(max(value, 0), 1) * 255).rounded()))
    }
}
#endif
