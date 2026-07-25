import SwiftUI

/// Chrome-less presentation of `WorkbenchLayoutMode.fullWindow`: the active
/// napplet fills the canvas edge-to-edge with no per-window title bar, and
/// opening another napplet pushes a new screen onto a standard iOS
/// navigation stack instead of adding a floating canvas window.
struct WorkbenchFullWindowView<WindowContent: View, TopBars: View>: View {
    @Binding var layout: WorkbenchLayoutModel
    let rootID: WorkbenchWindowID?
    @Binding var path: [WorkbenchWindowID]
    let onExit: () -> Void
    @ViewBuilder let windowContent: (WorkbenchCanvasWindow) -> WindowContent
    @ViewBuilder let topBars: () -> TopBars

    var body: some View {
        NavigationStack(path: $path) {
            screen(for: rootID.flatMap(layout.window(id:)))
                .navigationDestination(for: WorkbenchWindowID.self) { id in
                    screen(for: layout.window(id: id))
                }
        }
    }

    @ViewBuilder
    private func screen(for window: WorkbenchCanvasWindow?) -> some View {
        VStack(spacing: 0) {
            topBars()
            if let window {
                GeometryReader { proxy in
                    windowContent(window)
                        .padding(.top, proxy.safeAreaInsets.top)
                        .padding(.bottom, proxy.safeAreaInsets.bottom)
                }
                .ignoresSafeArea()
            } else {
                ContentUnavailableView {
                    Label("Your canvas is empty", systemImage: "macwindow")
                } description: {
                    Text("Choose Add Napplet to place one here.")
                }
            }
        }
        .background(.background)
        #if os(iOS)
        .toolbar(.hidden, for: .navigationBar)
        #endif
        .overlay(alignment: .topTrailing) {
            exitButton
        }
        .accessibilityIdentifier(
            window.map { "full-window-napplet-\($0.id.rawValue)" }
                ?? "full-window-empty"
        )
    }

    private var exitButton: some View {
        Button(action: onExit) {
            Image(systemName: "arrow.down.right.and.arrow.up.left")
                .font(.callout.weight(.semibold))
                .padding(10)
                .background(.thinMaterial, in: Circle())
        }
        .buttonStyle(.plain)
        .padding(.trailing, 14)
        .padding(.top, 8)
        .accessibilityLabel("Exit full window")
        .accessibilityHint("Returns to the multi-napplet canvas")
        .accessibilityIdentifier("exit-full-window")
    }
}
