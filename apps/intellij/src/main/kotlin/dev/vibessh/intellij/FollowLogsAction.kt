package dev.vibessh.intellij

import com.intellij.execution.filters.TextConsoleBuilderFactory
import com.intellij.execution.ui.ConsoleView
import com.intellij.execution.ui.ConsoleViewContentType
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.wm.ToolWindowAnchor
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.ui.content.ContentFactory

/**
 * View - Tool Windows - VibeSSH Logs, and the action that fills it.
 *
 * The piece that makes deploying to a server feel like running locally: the
 * output appears in the IDE, in a console tab, while it happens.
 *
 * **One stream at a time, and it is stopped before another starts.** The
 * connection holds a `docker logs -f` open on the far end; opening a second
 * console without closing the first would leave a process attached on the
 * server with nobody reading it.
 */
class FollowLogsAction : AnAction() {

    override fun actionPerformed(event: AnActionEvent) {
        val project = event.project ?: return
        val settings = VibeSshSettings.getInstance()
        if (settings.token.isBlank()) {
            Messages.showErrorDialog(project, "Najpierw wklej token w Ustawienia - Tools - VibeSSH.", "VibeSSH")
            return
        }

        ApplicationManager.getApplication().executeOnPooledThread {
            val applications = try {
                VibeSshClient(settings.baseUrl, settings.token).listApplications()
            } catch (err: Exception) {
                notify(project, err.message ?: "Nie udało się pobrać listy aplikacji.", NotificationType.ERROR)
                return@executeOnPooledThread
            }
            if (applications.isEmpty()) {
                notify(project, "VibeSSH nie ma żadnych aplikacji.", NotificationType.WARNING)
                return@executeOnPooledThread
            }
            ApplicationManager.getApplication().invokeLater {
                val labels = applications.map { it.toString() }.toTypedArray()
                val initial = applications.indexOfFirst { it.id == settings.applicationId }.coerceAtLeast(0)
                val index = Messages.showChooseDialog(project, "Czyje logi pokazać?", "VibeSSH", null, labels, labels[initial])
                val chosen = applications.getOrNull(index) ?: return@invokeLater
                settings.applicationId = chosen.id
                open(project, settings, chosen)
            }
        }
    }

    private fun open(project: Project, settings: VibeSshSettings, application: VibeSshClient.Application) {
        val service = LogConsoleService.getInstance(project)
        service.stop()

        val console = TextConsoleBuilderFactory.getInstance().createBuilder(project).console
        val window = ToolWindowManager.getInstance(project).getToolWindow(TOOL_WINDOW) ?: ToolWindowManager.getInstance(project)
            .registerToolWindow(TOOL_WINDOW) {
                anchor = ToolWindowAnchor.BOTTOM
                canCloseContent = true
            }

        window.contentManager.removeAllContents(true)
        val content = ContentFactory.getInstance().createContent(console.component, application.name, false)
        content.setDisposer(console)
        window.contentManager.addContent(content)
        window.activate(null)

        console.print("--- VibeSSH: podglad ${application.name}\n", ConsoleViewContentType.SYSTEM_OUTPUT)

        val stream = LogStream(settings.baseUrl, settings.token, application.id)
        service.start(stream, console)
    }

    private fun notify(project: Project, message: String, type: NotificationType) {
        ApplicationManager.getApplication().invokeLater {
            NotificationGroupManager.getInstance().getNotificationGroup("VibeSSH").createNotification(message, type).notify(project)
        }
    }

    companion object {
        const val TOOL_WINDOW = "VibeSSH Logs"
    }
}

/**
 * Owns the one running stream, so closing the project or opening another
 * console cannot leave a `docker logs -f` attached on a server forever.
 */
@com.intellij.openapi.components.Service(com.intellij.openapi.components.Service.Level.PROJECT)
class LogConsoleService : com.intellij.openapi.Disposable {
    private var stream: LogStream? = null

    fun start(stream: LogStream, console: ConsoleView) {
        this.stream = stream
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                stream.follow { line -> console.print("$line\n", ConsoleViewContentType.NORMAL_OUTPUT) }
                console.print("--- VibeSSH: strumien zakonczony\n", ConsoleViewContentType.SYSTEM_OUTPUT)
            } catch (err: Exception) {
                console.print("--- VibeSSH: ${err.message ?: err::class.simpleName}\n", ConsoleViewContentType.ERROR_OUTPUT)
            }
        }
    }

    fun stop() {
        stream?.close()
        stream = null
    }

    /** Called when the project closes - the last chance to hang up. */
    override fun dispose() = stop()

    companion object {
        fun getInstance(project: Project): LogConsoleService = project.getService(LogConsoleService::class.java)
    }
}
