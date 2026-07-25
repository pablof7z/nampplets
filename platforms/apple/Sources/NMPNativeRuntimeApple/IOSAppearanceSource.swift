#if os(iOS)
import Foundation
import NMPNativeRuntime
import UIKit

// MARK: - UIKit appearance projection

@MainActor
func keyWindow() -> UIWindow? {
    UIApplication.shared.connectedScenes
        .compactMap { $0 as? UIWindowScene }
        .flatMap(\.windows)
        .first(where: \.isKeyWindow)
}

private final class TraitObserverView: UIView {
    var onChange: (() -> Void)?

    override func traitCollectionDidChange(_ previousTraitCollection: UITraitCollection?) {
        super.traitCollectionDidChange(previousTraitCollection)
        guard let previousTraitCollection,
              previousTraitCollection.userInterfaceStyle != traitCollection.userInterfaceStyle
        else { return }
        onChange?()
    }
}

/// Event-driven projection of UIKit appearance facts. The callback reports
/// raw OS traits only; Rust maps them to the pinned NAP-THEME schema.
final class IOSAppearanceSource: NSObject, NativeAppearanceSource, @unchecked Sendable {
    private let lock = NSLock()
    private var snapshot: NativeAppearanceSnapshot
    private weak var controller: RuntimeController?
    private var isClosed = false
    private var refreshPending = false
    private var observerView: TraitObserverView?
    private var contrastObserver: NSObjectProtocol?
    private var transparencyObserver: NSObjectProtocol?

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
        guard observerView == nil else { return }
        guard let window = keyWindow() else { return }
        let view = TraitObserverView(frame: .zero)
        view.isHidden = true
        view.onChange = { [weak self] in
            self?.scheduleRefresh()
        }
        window.addSubview(view)
        observerView = view
        contrastObserver = NotificationCenter.default.addObserver(
            forName: UIAccessibility.darkerSystemColorsStatusDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.scheduleRefresh()
        }
        transparencyObserver = NotificationCenter.default.addObserver(
            forName: UIAccessibility.reduceTransparencyStatusDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.scheduleRefresh()
        }
    }

    @MainActor
    private func removeObservers() {
        observerView?.removeFromSuperview()
        observerView = nil
        if let contrastObserver {
            NotificationCenter.default.removeObserver(contrastObserver)
            self.contrastObserver = nil
        }
        if let transparencyObserver {
            NotificationCenter.default.removeObserver(transparencyObserver)
            self.transparencyObserver = nil
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
        let style = keyWindow()?.traitCollection.userInterfaceStyle ?? .light
        let dark = style == .dark
        let accent = UIColor.systemBlue.resolvedColor(
            with: UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
        )
        var red: CGFloat = 0
        var green: CGFloat = 0
        var blue: CGFloat = 0
        var alpha: CGFloat = 0
        accent.getRed(&red, green: &green, blue: &blue, alpha: &alpha)
        return NativeAppearanceSnapshot(
            dark: dark,
            increasedContrast: UIAccessibility.isDarkerSystemColorsEnabled,
            reducedTransparency: UIAccessibility.isReduceTransparencyEnabled,
            accentRed: component(red),
            accentGreen: component(green),
            accentBlue: component(blue)
        )
    }

    private static func component(_ value: CGFloat) -> UInt8 {
        UInt8(clamping: Int((min(max(value, 0), 1) * 255).rounded()))
    }
}
#endif
