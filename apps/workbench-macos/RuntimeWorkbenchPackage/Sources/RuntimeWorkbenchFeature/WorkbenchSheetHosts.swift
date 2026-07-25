import SwiftUI

/// Presents the exact-build activity drawer, or a truthful unavailable
/// fallback when no activity source or scope was admitted.
struct ActivitySheetHost: View {
    let source: RuntimeWorkbenchActivitySource?
    let scope: ActivityExactBuildScope?
    let error: String?

    var body: some View {
        if
            let source,
            let scope
        {
            ActivityDrawer(
                source: source,
                scope: scope
            )
        } else {
            NavigationStack {
                ContentUnavailableView(
                    "Activity unavailable",
                    systemImage: "waveform.path.ecg.rectangle",
                    description: Text(
                        error
                            ?? "The exact-build activity source was not admitted."
                    )
                )
                .navigationTitle("Runtime Activity")
                #if os(macOS)
                .frame(minWidth: 620, minHeight: 420)
                #endif
            }
        }
    }
}

/// Presents the permission review sheet, or a truthful unavailable fallback
/// when no permission manager was admitted.
struct PermissionSheetHost: View {
    let manager: (any PermissionReviewManaging)?
    let error: String?

    var body: some View {
        if let manager {
            PermissionReviewSheet(manager: manager)
        } else {
            NavigationStack {
                ContentUnavailableView(
                    "Permission review unavailable",
                    systemImage: "lock.slash",
                    description: Text(
                        error
                            ?? "The exact-build permission review was not admitted."
                    )
                )
                .navigationTitle("Review Permissions")
                #if os(macOS)
                .frame(minWidth: 620, minHeight: 420)
                #endif
            }
        }
    }
}

/// Presents the settings sheet, or a truthful unavailable fallback when no
/// settings snapshot was captured.
struct SettingsSheetHost: View {
    let snapshot: WorkbenchSettingsSnapshot?
    let openDestination: (WorkbenchSettingsDestination) -> Void

    var body: some View {
        if let snapshot {
            WorkbenchSettingsSheet(
                snapshot: snapshot,
                openDestination: openDestination
            )
        } else {
            NavigationStack {
                ContentUnavailableView(
                    "Settings unavailable",
                    systemImage: "gearshape.fill",
                    description: Text(
                        "The bounded runtime profile status could not be displayed."
                    )
                )
                .navigationTitle("Settings")
                #if os(macOS)
                .frame(minWidth: 620, minHeight: 420)
                #endif
            }
        }
    }
}
