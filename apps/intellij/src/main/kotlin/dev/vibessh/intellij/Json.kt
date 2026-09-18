package dev.vibessh.intellij

import com.google.gson.JsonArray
import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser

/**
 * Reading the endpoint's answers without a schema.
 *
 * **Everything here tolerates the shape being wrong.** These responses come
 * over a socket from another program that updates on its own schedule, and a
 * plugin that threw a `ClassCastException` into the IDE's error reporter
 * because a field moved would be worse than one that says it could not find
 * what it wanted. Every accessor returns null rather than raising, and the
 * callers turn that into a sentence.
 *
 * Gson because the IntelliJ platform already ships it - a plugin that brought
 * its own JSON library would be a plugin with a dependency to keep in step
 * with every IDE release.
 */
object Json {

    /** Walks a path of member names, with array indices written as digits. */
    private fun at(root: JsonElement?, vararg path: String): JsonElement? {
        var current: JsonElement? = root
        for (step in path) {
            current = when {
                current is JsonObject -> current.get(step)
                current is JsonArray -> step.toIntOrNull()?.let { if (it in 0 until current.size()) current[it] else null }
                else -> null
            }
            if (current == null || current.isJsonNull) return null
        }
        return current
    }

    private fun parse(text: String): JsonElement? =
        runCatching { JsonParser.parseString(text) }.getOrNull()

    fun objectAt(text: String, vararg path: String): String? =
        at(parse(text), *path)?.takeIf { it.isJsonObject }?.toString()

    fun stringAt(text: String, vararg path: String): String? {
        val found = at(parse(text), *path) ?: return null
        return when {
            found.isJsonPrimitive -> found.asString
            else -> null
        }
    }

    /**
     * The rows `list_applications` returns, as the plugin needs them.
     *
     * A row missing its id is skipped rather than shown: the id is the only
     * part the deploy call actually uses, so a row without one is an entry
     * that would fail the moment somebody selected it.
     */
    fun applications(text: String): List<VibeSshClient.Application> {
        val root = parse(text)
        if (root !is JsonArray) return emptyList()
        return root.mapNotNull { element ->
            val row = element as? JsonObject ?: return@mapNotNull null
            val id = row.get("id")?.takeIf { it.isJsonPrimitive }?.asString ?: return@mapNotNull null
            VibeSshClient.Application(
                id = id,
                name = row.get("name")?.takeIf { it.isJsonPrimitive }?.asString ?: id,
                status = row.get("status")?.takeIf { it.isJsonPrimitive }?.asString ?: "?",
                server = row.get("server_id")?.takeIf { it.isJsonPrimitive }?.asString,
            )
        }
    }
}
