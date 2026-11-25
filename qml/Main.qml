import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import l_tasktabs

ApplicationWindow {
    id: root

    width: 720
    height: 480
    minimumWidth: 480
    minimumHeight: 320
    visible: true
    title: AppController.appName()
    color: palette.window

    // Hide to tray instead of quitting (MVP 1).
    onClosing: function (close) {
        close.accepted = false
        root.hide()
        AppController.hideMainWindow()
    }

    Connections {
        target: AppController

        function onWindowVisibilityChanged(visible) {
            if (visible) {
                root.show()
                root.raise()
                root.requestActivate()
            } else {
                root.hide()
            }
        }

        function onQuitRequested() {
            Qt.quit()
        }
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            spacing: 8

            Label {
                text: AppController.appName()
                font.bold: true
                Layout.fillWidth: true
            }

            Label {
                text: "MVP 1 — shell"
                opacity: 0.6
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 12

        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: qsTr("L-TaskTabs is running. Close this window to hide to the tray; left-click the tray icon to show it again. Right-click the tray for the menu.")
        }

        Frame {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 12
                spacing: 8

                Label {
                    text: qsTr("Sessions")
                    font.bold: true
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    opacity: 0.7
                    text: qsTr("Tabbed builtin terminals arrive in a later MVP. This build validates Qt Bridge, QML, StatusNotifier tray, and single-instance.")
                }

                Item { Layout.fillHeight: true }
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 8

            Button {
                text: qsTr("Hide to tray")
                onClicked: {
                    root.hide()
                    AppController.hideMainWindow()
                }
            }
            Button {
                text: qsTr("Quit")
                onClicked: Qt.quit()
            }
        }
    }

    Component.onCompleted: {
        AppController.startTray()
        AppController.showMainWindow()
    }
}
