import SwiftUI

struct WorkbenchWorkspaceView<WindowContent: View>: View {
    @Binding var layout: WorkbenchLayoutModel
    let onLayoutChange: () -> Void
    let onClose: (WorkbenchCanvasWindow) -> Void
    @ViewBuilder let windowContent: (WorkbenchCanvasWindow) -> WindowContent

    private let canvasPadding = 12.0
    private let tileSpacing = 12.0

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .topLeading) {
                canvasBackground

                if layout.windows.isEmpty {
                    ContentUnavailableView {
                        Label("Your canvas is empty", systemImage: "macwindow")
                    } description: {
                        Text("Choose Add Napplet to place one here.")
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ForEach(layout.windows) { window in
                        canvasWindow(
                            window,
                            canvasSize: proxy.size
                        )
                    }
                }
            }
            .coordinateSpace(name: "workbench-canvas")
            .contentShape(Rectangle())
            .onTapGesture {
                var next = layout
                next.select(nil)
                guard next != layout else {
                    return
                }
                layout = next
                onLayoutChange()
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Napplet canvas")
        .accessibilityIdentifier("napplet-canvas")
    }

    private var canvasBackground: some View {
        Rectangle()
            .fill(.background)
            .overlay {
                Canvas { context, size in
                    let spacing = 24.0
                    var path = Path()
                    var x = spacing
                    while x < size.width {
                        var y = spacing
                        while y < size.height {
                            path.addEllipse(
                                in: CGRect(
                                    x: x,
                                    y: y,
                                    width: 1.25,
                                    height: 1.25
                                )
                            )
                            y += spacing
                        }
                        x += spacing
                    }
                    context.fill(
                        path,
                        with: .color(.secondary.opacity(0.16))
                    )
                }
                .allowsHitTesting(false)
            }
    }

    private func canvasWindow(
        _ window: WorkbenchCanvasWindow,
        canvasSize: CGSize
    ) -> some View {
        let frame = renderedFrame(
            for: window,
            canvasSize: canvasSize
        )
        return WorkbenchNappletWindow(
            window: window,
            frame: frame,
            isSelected: layout.snapshot.selectedWindowID == window.id,
            isFreeform: layout.mode == .freeform,
            content: { windowContent(window) },
            select: {
                var next = layout
                next.bringToFront(window.id)
                guard next != layout else {
                    return
                }
                layout = next
                onLayoutChange()
            },
            move: { origin, translation in
                guard layout.mode == .freeform else {
                    return
                }
                var next = layout
                next.moveWindow(
                    id: window.id,
                    x: origin.x + translation.width,
                    y: origin.y + translation.height,
                    canvasSize: canvasSize
                )
                layout = next
            },
            resize: { origin, translation in
                guard layout.mode == .freeform else {
                    return
                }
                var next = layout
                next.resizeWindow(
                    id: window.id,
                    width: origin.width + translation.width,
                    height: origin.height + translation.height,
                    canvasSize: canvasSize
                )
                layout = next
            },
            close: {
                onClose(window)
            },
            commitLayout: onLayoutChange
        )
        .frame(width: frame.width, height: frame.height)
        .position(
            x: frame.minX + frame.width / 2,
            y: frame.minY + frame.height / 2
        )
        .zIndex(Double(window.stackingOrder))
    }

    private func renderedFrame(
        for window: WorkbenchCanvasWindow,
        canvasSize: CGSize
    ) -> CGRect {
        switch layout.mode {
        case .freeform:
            let fitted = window.frame.fitted(to: canvasSize)
            return CGRect(
                x: fitted.x,
                y: fitted.y,
                width: fitted.width,
                height: fitted.height
            )
        case .tiling, .fullWindow:
            // `.fullWindow` is presented by a dedicated chrome-less screen on
            // iOS (see WorkbenchFullWindowView); this tiling fallback only
            // applies if the freeform/tiling canvas ever renders a workspace
            // synced from a device that saved that mode.
            let ordered = layout.windows
            guard
                let index = ordered.firstIndex(where: { $0.id == window.id })
            else {
                return .zero
            }
            let count = ordered.count
            let columns = max(Int(ceil(sqrt(Double(count)))), 1)
            let rows = max(Int(ceil(Double(count) / Double(columns))), 1)
            let usableWidth = max(
                canvasSize.width
                    - canvasPadding * 2
                    - tileSpacing * Double(columns - 1),
                WorkbenchWindowFrame.minimumWidth
            )
            let usableHeight = max(
                canvasSize.height
                    - canvasPadding * 2
                    - tileSpacing * Double(rows - 1),
                WorkbenchWindowFrame.minimumHeight
            )
            let width = usableWidth / Double(columns)
            let height = usableHeight / Double(rows)
            let column = index % columns
            let row = index / columns
            return CGRect(
                x: canvasPadding + Double(column) * (width + tileSpacing),
                y: canvasPadding + Double(row) * (height + tileSpacing),
                width: width,
                height: height
            )
        }
    }
}

private struct WorkbenchNappletWindow<Content: View>: View {
    let window: WorkbenchCanvasWindow
    let frame: CGRect
    let isSelected: Bool
    let isFreeform: Bool
    @ViewBuilder let content: () -> Content
    let select: () -> Void
    let move: (CGPoint, CGSize) -> Void
    let resize: (CGSize, CGSize) -> Void
    let close: () -> Void
    let commitLayout: () -> Void

    @State private var dragOrigin: CGPoint?
    @State private var resizeOrigin: CGSize?

    var body: some View {
        VStack(spacing: 0) {
            windowBar
            content()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .clipped()
        }
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .overlay {
            RoundedRectangle(cornerRadius: 10)
                .stroke(
                    isSelected
                        ? Color.accentColor
                        : Color.secondary.opacity(0.28),
                    lineWidth: isSelected ? 2 : 1
                )
                .allowsHitTesting(false)
        }
        .shadow(
            color: .black.opacity(isSelected ? 0.18 : 0.1),
            radius: isSelected ? 12 : 7,
            y: 3
        )
        .overlay(alignment: .bottomTrailing) {
            if isFreeform {
                resizeHandle
            }
        }
        .contentShape(Rectangle())
        .simultaneousGesture(
            TapGesture().onEnded {
                select()
            }
        )
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(window.title) napplet window")
        .accessibilityHint(
            isFreeform
                ? "Drag the title bar to move and the bottom-right handle to resize."
                : "Switch to Freeform layout to move or resize this window."
        )
        .accessibilityIdentifier("napplet-window-\(window.id.rawValue)")
    }

    private var windowBar: some View {
        HStack(spacing: 8) {
            Image(systemName: "app.dashed")
                .foregroundStyle(.tint)
            Text(window.title)
                .font(.headline)
                .lineLimit(1)
            Spacer()
            if isSelected {
                Image(systemName: "circle.fill")
                    .font(.system(size: 7))
                    .foregroundStyle(.tint)
                    .accessibilityHidden(true)
            }
            Button(action: close) {
                Image(systemName: "xmark.circle.fill")
            }
            .buttonStyle(.borderless)
            .foregroundStyle(.secondary)
            .accessibilityLabel("Close \(window.title)")
            .accessibilityHint(
                "Closes this window without uninstalling the napplet"
            )
        }
        .padding(.horizontal, 11)
        .frame(height: 38)
        .background(.bar)
        .contentShape(Rectangle())
        .gesture(moveGesture)
        .accessibilityLabel("\(window.title) title bar")
    }

    private var moveGesture: some Gesture {
        DragGesture(
            minimumDistance: 2,
            coordinateSpace: .named("workbench-canvas")
        )
        .onChanged { value in
            guard isFreeform else {
                return
            }
            if dragOrigin == nil {
                dragOrigin = frame.origin
                select()
            }
            guard let dragOrigin else {
                return
            }
            move(dragOrigin, value.translation)
        }
        .onEnded { _ in
            guard dragOrigin != nil else {
                return
            }
            dragOrigin = nil
            commitLayout()
        }
    }

    private var resizeHandle: some View {
        Image(systemName: "arrow.down.right.and.arrow.up.left")
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)
            .frame(width: 26, height: 26)
            .contentShape(Rectangle())
            .gesture(resizeGesture)
            .accessibilityLabel("Resize \(window.title)")
            .accessibilityHint("Drag to resize this napplet window")
    }

    private var resizeGesture: some Gesture {
        DragGesture(minimumDistance: 1)
            .onChanged { value in
                if resizeOrigin == nil {
                    resizeOrigin = frame.size
                    select()
                }
                guard let resizeOrigin else {
                    return
                }
                resize(resizeOrigin, value.translation)
            }
            .onEnded { _ in
                guard resizeOrigin != nil else {
                    return
                }
                resizeOrigin = nil
                commitLayout()
            }
    }
}
