import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

Panel {
    id: root
    moduleName: "io.github.kauanmassuia14.portpilot"
    ipcTarget: "io.github.kauanmassuia14.portpilot"
    manageIpc: false

    property var anchorItem: null
    property var hostWidget: null
    property string page: "list"
    property int selectedIndex: 0
    property string selectedServiceKey: ""
    property int actionIndex: 0
    property string actionMessage: ""
    property double openedAt: 0

    readonly property var allServices: hostWidget ? hostWidget.services : []
    readonly property var visibleServices: allServices.filter(function(service) {
        return service && service.class === "development"
    })
    readonly property var selectedService: {
        for (var i = 0; i < visibleServices.length; i++)
            if (serviceKey(visibleServices[i]) === selectedServiceKey) return visibleServices[i]
        return null
    }
    readonly property var detailRows: {
        if (!selectedService) return []
        var rows = [
            { label: "Project", value: serviceProject(selectedService) },
            { label: "Port", value: String(selectedService.port) }
        ]
        if (selectedService.docker) {
            rows.push({ label: "Container", value: selectedService.docker.container })
            rows.push({ label: "Compose", value: selectedService.docker.compose_project || "—" })
            rows.push({ label: "Service", value: selectedService.docker.compose_service || "—" })
            rows.push({ label: "Image", value: selectedService.docker.image })
        } else {
            rows.push({ label: "Process", value: selectedService.process_name })
            rows.push({ label: "PID", value: selectedService.pid === null ? "—" : String(selectedService.pid) })
            rows.push({ label: "Command", value: selectedService.command || "—" })
            if (selectedService.user) rows.push({ label: "User", value: selectedService.user })
            if (selectedService.project && selectedService.project.git_branch)
                rows.push({ label: "Git", value: selectedService.project.git_branch })
        }
        rows.push({ label: "Root", value: serviceRoot(selectedService) })
        rows.push({ label: "URL", value: selectedService.url || "—" })
        return rows
    }
    readonly property var selectedActions: {
        var items = []
        if (!selectedService) return items
        if (selectedService.url) items.push("Open in browser")
        if (selectedService.project && selectedService.project.root) {
            items.push("Open terminal")
            items.push("Open editor")
        }
        items.push("Stop service")
        return items
    }
    readonly property color foreground: bar ? bar.foreground : Color.foreground
    readonly property color muted: Qt.rgba(foreground.r, foreground.g, foreground.b, 0.58)

    function open() {
        openedAt = Date.now()
        page = "list"
        actionMessage = ""
        if (hostWidget) hostWidget.refreshServices()
        controller.show()
    }
    function close() {
        if (Date.now() - openedAt < 280) return
        controller.hide()
    }
    function toggle() { opened ? close() : open() }

    function moveCursor(delta) {
        if (page === "list" && visibleServices.length > 0)
            selectedIndex = Math.max(0, Math.min(visibleServices.length - 1, selectedIndex + delta))
        else if (page === "actions" && selectedActions.length > 0)
            actionIndex = Math.max(0, Math.min(selectedActions.length - 1, actionIndex + delta))
    }
    function serviceKey(service) {
        if (!service) return ""
        if (service.docker) return String(service.port) + ":docker:" + String(service.docker.container_id)
        return String(service.port) + ":process:" + String(service.process_start_time || service.pid || service.process_name)
    }
    function activate() {
        if (page === "list") {
            if (visibleServices.length > 0) chooseService(selectedIndex)
        } else if (page === "details") {
            page = "actions"
            actionIndex = 0
        } else if (page === "actions") {
            activateAction(actionIndex)
        } else if (page === "confirm") {
            stopSelectedService()
        } else if (page === "result") {
            page = "list"
        }
    }
    function chooseService(index) {
        if (index < 0 || index >= visibleServices.length) return
        selectedIndex = index
        selectedServiceKey = serviceKey(visibleServices[index])
        page = "details"
    }
    function startAction(args, message) {
        if (actionProcess.running) return
        actionMessage = message
        actionProcess.command = args
        actionProcess.running = true
    }
    function activateAction(index) {
        if (!selectedService || index < 0 || index >= selectedActions.length) return
        var action = selectedActions[index]
        var port = String(selectedService.port)
        if (action === "Open in browser") startAction([hostWidget.binary, "open", port], "Opening local service…")
        else if (action === "Open terminal") startAction([hostWidget.binary, "terminal", port], "Opening project terminal…")
        else if (action === "Open editor") startAction([hostWidget.binary, "edit", port], "Opening project in editor…")
        else if (action === "Stop service") page = "confirm"
    }
    function stopSelectedService() {
        if (!selectedService) return
        var command = [hostWidget.binary, "kill", String(selectedService.port), "--yes"]
        if (selectedService.docker) {
            command.push("--expect-container", String(selectedService.docker.container_id))
        } else {
            command.push("--expect-pid", String(selectedService.pid))
            command.push("--expect-name", String(selectedService.process_name))
            if (selectedService.process_start_time !== null && selectedService.process_start_time !== undefined)
                command.push("--expect-start-time", String(selectedService.process_start_time))
        }
        startAction(command, "Stopping service…")
    }
    function goBack() {
        if (page === "list") close()
        else if (page === "actions") page = "details"
        else if (page === "confirm") page = "actions"
        else if (page === "result") page = "list"
        else page = "list"
    }
    onSelectedServiceChanged: {
        if (!selectedService && (page === "details" || page === "actions")) page = "list"
    }
    function serviceProject(service) {
        if (!service) return "unknown"
        return service.project ? service.project.name : "unknown"
    }
    function serviceRoot(service) {
        if (!service) return "—"
        return service.project && service.project.root ? service.project.root : (service.cwd || "—")
    }

    Process {
        id: actionProcess
        stdout: StdioCollector { waitForEnd: true }
        stderr: StdioCollector { id: actionError; waitForEnd: true }
        onExited: function(exitCode) {
            if (exitCode !== 0) actionMessage = String(actionError.text || "Action failed").trim()
            else if (actionMessage === "Stopping service…") actionMessage = "Service stopped gracefully."
            else actionMessage = "Done."
            page = "result"
            if (hostWidget) hostWidget.refreshServices()
        }
    }

    Timer {
        interval: root.hostWidget ? root.hostWidget.refreshIntervalSec * 1000 : 3000
        repeat: true
        running: root.opened
        onTriggered: if (root.hostWidget) root.hostWidget.refreshServices()
    }

    KeyboardPanel {
        id: panel
        anchorItem: root.anchorItem
        owner: root.hostWidget || root
        bar: root.bar
        open: root.opened
        focusTarget: keyCatcher
        contentWidth: Style.space(520)
        contentHeight: Style.space(440)

        PanelKeyCatcher {
            id: keyCatcher
            anchors.fill: parent
            onMoveRequested: function(dx, dy) { if (dy !== 0) root.moveCursor(dy) }
            onActivateRequested: root.activate()
            onReturnRequested: {}
            onCloseRequested: root.close()

            Column {
                anchors.fill: parent
                anchors.margins: Style.space(18)
                spacing: Style.space(12)

                Row {
                    width: parent.width
                    spacing: Style.space(10)
                    Column {
                        width: parent.width - refreshButton.width - Style.space(10)
                        Text {
                            textFormat: Text.PlainText
                            text: "PortPilot"
                            color: root.foreground
                            font.family: Style.font.family
                            font.pixelSize: Style.font.title
                            font.bold: true
                        }
                        Text {
                            textFormat: Text.PlainText
                            text: root.visibleServices.length + " local services"
                            color: root.muted
                            font.family: Style.font.family
                            font.pixelSize: Style.font.caption
                        }
                    }
                    Rectangle {
                        id: refreshButton
                        width: Style.space(34); height: Style.space(34); radius: Style.cornerRadius
                        color: refreshMouse.containsMouse ? Style.hoverFillFor(root.foreground, Color.accent) : "transparent"
                        Text { anchors.centerIn: parent; text: "↻"; color: root.foreground; font.pixelSize: Style.font.title }
                        MouseArea { id: refreshMouse; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: if (hostWidget) hostWidget.refreshServices() }
                    }
                }

                Rectangle { width: parent.width; height: 1; color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.12) }

                ListView {
                    id: serviceList
                    visible: root.page === "list"
                    width: parent.width
                    height: Math.max(Style.space(80), Math.min(contentHeight, Style.space(300)))
                    clip: true
                    model: root.visibleServices
                    currentIndex: root.selectedIndex
                    onCurrentIndexChanged: if (currentIndex >= 0 && currentIndex < count) positionViewAtIndex(currentIndex, ListView.Contain)
                    delegate: Rectangle {
                        required property int index
                        required property var modelData
                        width: serviceList.width
                        height: Style.space(54)
                        radius: Style.cornerRadius
                        color: index === root.selectedIndex
                            ? Style.selectedFillFor(root.foreground, Color.accent)
                            : serviceMouse.containsMouse ? Style.hoverFillFor(root.foreground, Color.accent) : "transparent"
                        Row {
                            anchors.fill: parent
                            anchors.margins: Style.space(10)
                            spacing: Style.space(10)
                            Text {
                                width: Style.space(54)
                                textFormat: Text.PlainText
                                text: String(modelData.port)
                                color: Color.accent
                                font.family: Style.font.family
                                font.pixelSize: Style.font.body
                                font.bold: true
                                anchors.verticalCenter: parent.verticalCenter
                            }
                            Column {
                                width: parent.width - Style.space(114)
                                anchors.verticalCenter: parent.verticalCenter
                                Text {
                                    width: parent.width
                                    textFormat: Text.PlainText
                                    text: root.serviceProject(modelData)
                                    color: root.foreground
                                    font.family: Style.font.family
                                    font.pixelSize: Style.font.body
                                    elide: Text.ElideRight
                                }
                                Text {
                                    width: parent.width
                                    textFormat: Text.PlainText
                                    text: modelData.docker ? modelData.docker.container : modelData.process_name
                                    color: root.muted
                                    font.family: Style.font.family
                                    font.pixelSize: Style.font.caption
                                    elide: Text.ElideRight
                                }
                            }
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                textFormat: Text.PlainText
                                text: "●"
                                color: "#3fb950"
                                font.pixelSize: Style.font.caption
                            }
                        }
                        MouseArea {
                            id: serviceMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.chooseService(index)
                        }
                    }
                    Text {
                        anchors.centerIn: parent
                        visible: root.visibleServices.length === 0
                        textFormat: Text.PlainText
                        text: "No local development services found."
                        color: root.muted
                        font.family: Style.font.family
                        font.pixelSize: Style.font.body
                    }
                }

                Column {
                    visible: root.page === "details"
                    width: parent.width
                    spacing: Style.space(8)
                    Text {
                        textFormat: Text.PlainText
                        text: "‹  Back to services"
                        color: Color.accent
                        font.family: Style.font.family
                        font.pixelSize: Style.font.caption
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.page = "list" }
                    }
                    Text {
                        textFormat: Text.PlainText
                        text: root.selectedService ? root.serviceProject(root.selectedService) : "Service details"
                        color: root.foreground
                        font.family: Style.font.family
                        font.pixelSize: Style.font.title
                        font.bold: true
                    }
                    Repeater {
                        model: root.detailRows
                        delegate: Row {
                            required property var modelData
                            width: parent.width
                            spacing: Style.space(8)
                            Text { width: Style.space(86); textFormat: Text.PlainText; text: modelData.label; color: root.muted; font.family: Style.font.family; font.pixelSize: Style.font.caption }
                            Text { width: parent.width - Style.space(94); textFormat: Text.PlainText; text: modelData.value; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.caption; elide: Text.ElideMiddle; wrapMode: Text.WrapAnywhere }
                        }
                    }
                    Rectangle {
                        width: Style.space(150); height: Style.space(38); radius: Style.cornerRadius
                        color: actionButton.containsMouse ? Style.hoverFillFor(root.foreground, Color.accent) : "transparent"
                        border.color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.2)
                        Text { anchors.centerIn: parent; text: "Choose action"; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.body }
                        MouseArea { id: actionButton; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: { root.page = "actions"; root.actionIndex = 0 } }
                    }
                }

                Column {
                    visible: root.page === "actions"
                    width: parent.width
                    spacing: Style.space(8)
                    Text { textFormat: Text.PlainText; text: "‹  Service details"; color: Color.accent; font.family: Style.font.family; font.pixelSize: Style.font.caption; MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.page = "details" } }
                    Text { textFormat: Text.PlainText; text: root.selectedService ? root.serviceProject(root.selectedService) : "Actions"; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.title; font.bold: true }
                    Repeater {
                        model: root.selectedActions
                        delegate: Rectangle {
                            required property int index
                            required property string modelData
                            width: parent.width
                            height: Style.space(42)
                            radius: Style.cornerRadius
                            color: index === root.actionIndex ? Style.selectedFillFor(root.foreground, Color.accent) : actionMouse.containsMouse ? Style.hoverFillFor(root.foreground, Color.accent) : "transparent"
                            Text { anchors.left: parent.left; anchors.leftMargin: Style.space(12); anchors.verticalCenter: parent.verticalCenter; textFormat: Text.PlainText; text: modelData; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.body }
                            MouseArea { id: actionMouse; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: { root.actionIndex = index; root.activateAction(index) } }
                        }
                    }
                }

                Column {
                    visible: root.page === "confirm"
                    width: parent.width
                    spacing: Style.space(10)
                    Text { textFormat: Text.PlainText; text: "Stop this service?"; color: Color.urgent; font.family: Style.font.family; font.pixelSize: Style.font.title; font.bold: true }
                    Text {
                        width: parent.width
                        textFormat: Text.PlainText
                        text: root.selectedService ? root.serviceProject(root.selectedService) + "\nPort " + root.selectedService.port + " · " + (root.selectedService.docker ? root.selectedService.docker.container : "PID " + root.selectedService.pid) + "\n" + (root.selectedService.command || root.selectedService.process_name) : "This service changed or closed. Refresh before selecting another."
                        color: root.foreground
                        font.family: Style.font.family
                        font.pixelSize: Style.font.body
                        wrapMode: Text.WrapAnywhere
                    }
                    Row {
                        spacing: Style.space(8)
                        Rectangle { width: Style.space(112); height: Style.space(38); radius: Style.cornerRadius; color: cancelMouse.containsMouse ? Style.hoverFillFor(root.foreground, Color.accent) : "transparent"; Text { anchors.centerIn: parent; text: "Cancel"; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.body }; MouseArea { id: cancelMouse; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: root.page = "actions" } }
                        Rectangle { width: Style.space(152); height: Style.space(38); radius: Style.cornerRadius; color: root.selectedService ? Color.urgent : Qt.darker(Color.urgent, 1.8); Text { anchors.centerIn: parent; text: "Stop with SIGTERM"; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.caption }; MouseArea { anchors.fill: parent; enabled: !!root.selectedService; cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor; onClicked: root.stopSelectedService() } }
                    }
                }

                Column {
                    visible: root.page === "result"
                    width: parent.width
                    spacing: Style.space(8)
                    Text { textFormat: Text.PlainText; text: "PortPilot"; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.title; font.bold: true }
                    Text { width: parent.width; textFormat: Text.PlainText; text: root.actionMessage; color: root.foreground; font.family: Style.font.family; font.pixelSize: Style.font.body; wrapMode: Text.WrapAnywhere }
                    Text { textFormat: Text.PlainText; text: "‹  Back to services"; color: Color.accent; font.family: Style.font.family; font.pixelSize: Style.font.caption; MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.page = "list" } }
                }
            }
        }
    }
}
