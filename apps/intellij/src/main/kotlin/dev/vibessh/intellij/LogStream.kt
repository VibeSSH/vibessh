package dev.vibessh.intellij

import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.HttpURLConnection
import java.net.URI
import java.nio.charset.StandardCharsets

/**
 * Reading an application's output as it happens.
 *
 * A connection that is deliberately never expected to end: VibeSSH holds it
 * open and writes a line whenever the application does. So `readTimeout` is
 * zero - a server that has nothing to say for ten minutes is not a server
 * that has gone away, and a timeout here would close a console every time a
 * game server went quiet overnight.
 *
 * Stopping is the reader's job. `close()` shuts the socket, the endpoint
 * notices, and the `docker logs -f` it started on the far end stops with it -
 * which is why closing a console has to actually do this rather than just
 * hiding a window.
 */
class LogStream(
    private val baseUrl: String,
    private val token: String,
    private val applicationId: String,
    private val tail: Int = 200,
) {
    @Volatile
    private var connection: HttpURLConnection? = null

    @Volatile
    private var closed = false

    /**
     * Blocks, handing every line to `onLine`, until the stream ends or
     * [close] is called. Runs on a background thread; never call it on the
     * interface thread.
     */
    fun follow(onLine: (String) -> Unit) {
        val url = "$baseUrl/logs?application=$applicationId&tail=$tail"
        val open = (URI(url).toURL().openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"
            setRequestProperty("Authorization", "Bearer $token")
            connectTimeout = 5_000
            // Zero means "no timeout" - see the class comment.
            readTimeout = 0
        }
        connection = open

        val code = try {
            open.responseCode
        } catch (err: Exception) {
            if (closed) return
            throw VibeSshClient.VibeSshException(
                "Nie udało się połączyć z VibeSSH pod $baseUrl - czy aplikacja działa i czy endpoint jest włączony?",
                err,
            )
        }
        if (code == 401) throw VibeSshClient.VibeSshException("VibeSSH odrzucił token - skopiuj go ponownie z Ustawień")
        if (code !in 200..299) {
            val detail = open.errorStream?.readBytes()?.toString(StandardCharsets.UTF_8).orEmpty()
            throw VibeSshClient.VibeSshException("VibeSSH odpowiedział $code: ${detail.ifBlank { "(bez treści)" }}")
        }

        BufferedReader(InputStreamReader(open.inputStream, StandardCharsets.UTF_8)).use { reader ->
            while (true) {
                val line = try {
                    reader.readLine()
                } catch (err: Exception) {
                    // A closed socket throws on the read in progress. That is
                    // this object being stopped on purpose, not a fault.
                    if (closed) return else throw err
                } ?: return
                onLine(line)
            }
        }
    }

    fun close() {
        closed = true
        runCatching { connection?.disconnect() }
    }
}
