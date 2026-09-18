package dev.vibessh.intellij

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.Messages
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBPasswordField
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JButton
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * Settings - Tools - VibeSSH.
 *
 * **The token field is a password field even though it is the user's own
 * token.** This page gets opened during screen shares and screenshots -
 * that is how the first one very nearly got away - and there is no reason
 * to read it back once it is set.
 *
 * **"Test connection" performs the MCP handshake** rather than pinging the
 * port, because that answers all three questions somebody has at once: is
 * VibeSSH running, is the endpoint switched on, and is this token right. A
 * port that accepts a connection answers none of them.
 */
class VibeSshConfigurable : Configurable {
    private val portField = JBTextField()
    private val tokenField = JBPasswordField()
    private val directoryField = JBTextField()
    private val restartBox = JBCheckBox("Zrestartuj aplikację po wgraniu")
    private var panel: JPanel? = null

    override fun getDisplayName(): String = "VibeSSH"

    override fun createComponent(): JComponent {
        val settings = VibeSshSettings.getInstance()
        portField.text = settings.port.toString()
        tokenField.text = settings.token
        directoryField.text = settings.remoteDirectory
        restartBox.isSelected = settings.restartAfterDeploy

        val test = JButton("Testuj połączenie").apply { addActionListener { testConnection() } }

        panel = FormBuilder.createFormBuilder()
            .addLabeledComponent("Port", portField)
            .addComponentToRightColumn(hint("VibeSSH nasłuchuje na 127.0.0.1. Adresu nie da się zmienić - i po drugiej stronie też nie."))
            .addLabeledComponent("Token", tokenField)
            .addComponentToRightColumn(hint("Z Ustawień VibeSSH: Claude i inni asystenci. Trafia do magazynu haseł IDE, nie do plików projektu."))
            .addLabeledComponent("Katalog docelowy", directoryField)
            .addComponentToRightColumn(hint("Względem katalogu roboczego aplikacji, np. plugins."))
            .addComponent(restartBox)
            .addComponent(test)
            .addComponentFillVertically(JPanel(), 0)
            .panel
        return panel!!
    }

    /** The grey explanatory line under a field, in the platform's own colour
     *  for exactly this - not a colour picked here, so it follows the theme. */
    private fun hint(text: String) = com.intellij.ui.components.JBLabel(text).apply {
        foreground = com.intellij.util.ui.UIUtil.getContextHelpForeground()
    }

    /**
     * Run off the interface thread: it opens a socket, and an IDE that
     * freezes because a port did not answer within five seconds is an IDE
     * somebody force-quits.
     */
    private fun testConnection() {
        val port = portField.text.trim().toIntOrNull()
        if (port == null || port !in 1..65535) {
            Messages.showErrorDialog("Port musi być liczbą od 1 do 65535.", "VibeSSH")
            return
        }
        val token = String(tokenField.password)
        ApplicationManager.getApplication().executeOnPooledThread {
            val message = try {
                val info = VibeSshClient("http://127.0.0.1:$port", token).handshake()
                "Połączono z ${info.name} ${info.version}."
            } catch (err: Exception) {
                err.message ?: "Nie udało się połączyć."
            }
            ApplicationManager.getApplication().invokeLater {
                Messages.showInfoMessage(message, "VibeSSH")
            }
        }
    }

    override fun isModified(): Boolean {
        val settings = VibeSshSettings.getInstance()
        return portField.text.trim() != settings.port.toString() ||
            String(tokenField.password) != settings.token ||
            directoryField.text.trim() != settings.remoteDirectory ||
            restartBox.isSelected != settings.restartAfterDeploy
    }

    override fun apply() {
        val settings = VibeSshSettings.getInstance()
        portField.text.trim().toIntOrNull()?.takeIf { it in 1..65535 }?.let { settings.port = it }
        settings.token = String(tokenField.password)
        settings.remoteDirectory = directoryField.text.trim().ifBlank { "plugins" }
        settings.restartAfterDeploy = restartBox.isSelected
    }

    override fun reset() {
        val settings = VibeSshSettings.getInstance()
        portField.text = settings.port.toString()
        tokenField.text = settings.token
        directoryField.text = settings.remoteDirectory
        restartBox.isSelected = settings.restartAfterDeploy
    }
}
