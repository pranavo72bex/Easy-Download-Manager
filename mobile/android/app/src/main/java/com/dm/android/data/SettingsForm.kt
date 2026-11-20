// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonObject

/** How an Options line is edited. */
enum class FieldKind { SWITCH, WHOLE_NUMBER, NUMBER, TEXT }

/**
 * One Options line: the keys leading to it in the settings' JSON, its words, how it is edited,
 * and its value as typed (`"true"`/`"false"` for a switch).
 */
data class SettingsField(
    val path: List<String>,
    val label: String,
    val kind: FieldKind,
    val value: String,
) {
    val key: String get() = path.joinToString("/")
}

/** A group of lines under a heading; the top level's heading is null. */
data class SettingsSection(val title: String?, val fields: List<SettingsField>)

/** A number field that does not read as one. */
class NotANumber(val label: String) : IllegalArgumentException(label)

/**
 * Options drawn from the engine's settings JSON with no second copy of each line (the
 * decision of 2026-11-20): every boolean a switch, every number a number field, every string a
 * text field. Lists and empty values are kept as they are and not shown.
 *
 * dm's settings are flat (dm-shell flattens Options' pages into one object), so most lines are
 * in the first section; a nested object (the slow hours, say) is a section of its own.
 */
object SettingsForm {
    private val json = Json { prettyPrint = false }

    fun read(text: String): List<SettingsSection> {
        val root = json.parseToJsonElement(text).jsonObject
        val top = mutableListOf<SettingsField>()
        val sections = mutableListOf<SettingsSection>()
        for ((key, value) in root) {
            when (value) {
                is JsonPrimitive -> fieldOf(listOf(key), value)?.let(top::add)
                is JsonObject -> {
                    val fields = mutableListOf<SettingsField>()
                    collect(listOf(key), value, fields)
                    if (fields.isNotEmpty()) sections += SettingsSection(Format.label(key), fields)
                }
                else -> Unit
            }
        }
        return listOf(SettingsSection(null, top)) + sections
    }

    /** `original` with each field's value put in; [NotANumber] for a number that is not one. */
    fun write(original: String, fields: List<SettingsField>): String {
        var root = json.parseToJsonElement(original).jsonObject
        for (field in fields) root = put(root, field.path, valueOf(field))
        return json.encodeToString(JsonElement.serializer(), root)
    }

    private fun collect(path: List<String>, obj: JsonObject, into: MutableList<SettingsField>) {
        for ((key, value) in obj) {
            when (value) {
                is JsonPrimitive -> fieldOf(path + key, value)?.let(into::add)
                is JsonObject -> collect(path + key, value, into)
                else -> Unit
            }
        }
    }

    private fun fieldOf(path: List<String>, value: JsonPrimitive): SettingsField? {
        val kind = kindOf(value) ?: return null
        // A section's lines are named without the section's own name, which heads them.
        val label = if (path.size == 1) Format.label(path.first())
        else path.drop(1).joinToString(" › ") { Format.label(it) }
        return SettingsField(path, label, kind, value.content)
    }

    private fun kindOf(value: JsonPrimitive): FieldKind? = when {
        value is JsonNull -> null
        value.isString -> FieldKind.TEXT
        value.booleanOrNull != null -> FieldKind.SWITCH
        value.content.toBigIntegerOrNull() != null -> FieldKind.WHOLE_NUMBER
        value.content.toDoubleOrNull() != null -> FieldKind.NUMBER
        else -> null
    }

    private fun valueOf(field: SettingsField): JsonPrimitive {
        val text = field.value.trim()
        return when (field.kind) {
            FieldKind.SWITCH -> JsonPrimitive(text.toBoolean())
            FieldKind.TEXT -> JsonPrimitive(field.value)
            FieldKind.WHOLE_NUMBER ->
                JsonPrimitive(text.toBigIntegerOrNull() ?: throw NotANumber(field.label))
            FieldKind.NUMBER -> JsonPrimitive(
                text.toDoubleOrNull()?.takeIf { it.isFinite() } ?: throw NotANumber(field.label),
            )
        }
    }

    private fun put(obj: JsonObject, path: List<String>, value: JsonElement): JsonObject {
        val key = path.first()
        val next: JsonElement = if (path.size == 1) {
            value
        } else {
            val inner = obj[key] as? JsonObject ?: return obj
            put(inner, path.drop(1), value)
        }
        return JsonObject(obj.toMutableMap().apply { put(key, next) })
    }
}
