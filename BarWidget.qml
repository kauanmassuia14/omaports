import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
    id: root
    moduleName: "io.github.kauanmassuia14.portpilot"

    readonly property string binary: String(setting("binary", "portpilot") || "portpilot")
    readonly property string icon: String(setting("icon", "󰖟") || "󰖟")
    readonly property int refreshIntervalSec: Math.max(3, Number(setting("refreshIntervalSec", 3)) || 3)
    property var services: []
    property int serviceCount: 0
    property string scanError: ""
    property string statusTooltip: "No local development services"
    readonly property string tooltipText: scanError !== "" ? "PortPilot: " + scanError : statusTooltip

    function refresh() {
        if (statusProcess.running) return
        statusProcess.command = [root.binary, "waybar"]
        statusProcess.running = true
    }

    function refreshServices() {
        if (serviceProcess.running) return
        serviceProcess.command = [root.binary, "list", "--json"]
        serviceProcess.running = true
    }

    function injectPanel() {
        if (!panelLoader.item) return
        panelLoader.item.bar = root.bar
        panelLoader.item.settings = root.settings
        panelLoader.item.anchorItem = button
        panelLoader.item.hostWidget = root
    }

    function togglePanel() {
        if (panelLoader.item) panelLoader.item.toggle()
    }

    Process {
        id: statusProcess
        stdout: StdioCollector { id: statusOutput; waitForEnd: true }
        stderr: StdioCollector { id: scanErrorOutput; waitForEnd: true }
        onExited: function(exitCode) {
            if (exitCode !== 0) {
                root.scanError = String(scanErrorOutput.text || "PortPilot could not scan services").trim()
                return
            }
            try {
                var status = JSON.parse(statusOutput.text || "{}")
                var count = String(status.text || "").match(/(\d+)\s*$/)
                root.serviceCount = count ? Number(count[1]) : 0
                root.statusTooltip = String(status.tooltip || "No local development services")
                root.scanError = ""
            } catch (error) {
                root.scanError = "PortPilot returned invalid status data"
            }
        }
    }

    Process {
        id: serviceProcess
        stdout: StdioCollector { id: servicesOutput; waitForEnd: true }
        stderr: StdioCollector { id: servicesErrorOutput; waitForEnd: true }
        onExited: function(exitCode) {
            if (exitCode !== 0) {
                root.scanError = String(servicesErrorOutput.text || "Could not refresh local services").trim()
                return
            }
            try {
                var result = JSON.parse(servicesOutput.text || "[]")
                root.services = result instanceof Array ? result : []
                root.scanError = ""
            } catch (error) {
                root.scanError = "PortPilot returned invalid service data"
            }
        }
    }

    Timer {
        interval: root.refreshIntervalSec * 1000
        repeat: true
        running: true
        onTriggered: root.refresh()
    }

    Loader {
        id: panelLoader
        active: true
        source: Qt.resolvedUrl("Panel.qml")
        visible: false
        onLoaded: Qt.callLater(root.injectPanel)
    }

    IpcHandler {
        target: "io.github.kauanmassuia14.portpilot"
        function open(): void { if (panelLoader.item) panelLoader.item.open() }
        function close(): void { if (panelLoader.item) panelLoader.item.close() }
        function toggle(): void { root.togglePanel() }
    }

    WidgetButton {
        id: button
        anchors.fill: parent
        bar: root.bar
        text: root.icon + " " + (root.scanError !== "" ? "!" : String(root.serviceCount))
        fontSize: Style.font.icon
        tooltipText: root.tooltipText
        onPressed: function(mouseButton) {
            if (mouseButton === Qt.LeftButton) root.togglePanel()
            else if (mouseButton === Qt.RightButton) root.refresh()
        }
    }

    implicitWidth: button.implicitWidth
    implicitHeight: button.implicitHeight
    Component.onCompleted: Qt.callLater(root.refresh)
    onBarChanged: injectPanel()
    onSettingsChanged: { injectPanel(); Qt.callLater(root.refresh) }
}
