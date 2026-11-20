// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.dm.android.R
import com.dm.android.data.FieldKind
import com.dm.android.data.SettingsField

/**
 * Options as one scrolling page (gaps row N7), drawn from the engine's settings JSON: every
 * line the desktop's Options has, without a second copy of each. Save puts all of it in force
 * or none, and shows the engine's refusal.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OptionsScreen(state: OptionsState, model: DownloadsViewModel, snackbar: SnackbarHostState) {
    BackHandler(onBack = model::closeOptions)
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.action_options)) },
                navigationIcon = {
                    IconButton(onClick = model::closeOptions) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.action_back))
                    }
                },
                actions = {
                    TextButton(onClick = model::saveOptions, enabled = !state.saving) {
                        Text(stringResource(R.string.options_save))
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let { error ->
                item(key = "error") {
                    Text(
                        error.words(),
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
            state.sections.forEach { section ->
                item(key = "section:${section.title}") {
                    Text(
                        section.title ?: stringResource(R.string.options_general),
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 4.dp),
                    )
                }
                items(section.fields, key = { it.key }) { field ->
                    FieldLine(field) { model.editOption(field.key, it) }
                }
            }
        }
    }
}

/** One line: a switch, a number field or a text field, by the value's kind. */
@Composable
private fun FieldLine(field: SettingsField, onChange: (String) -> Unit) {
    val modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)
    when (field.kind) {
        FieldKind.SWITCH -> Row(modifier, verticalAlignment = Alignment.CenterVertically) {
            Text(field.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Switch(checked = field.value == "true", onCheckedChange = { onChange(it.toString()) })
        }
        FieldKind.WHOLE_NUMBER, FieldKind.NUMBER -> OutlinedTextField(
            value = field.value,
            onValueChange = onChange,
            label = { Text(field.label) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = if (field.kind == FieldKind.NUMBER) KeyboardType.Decimal else KeyboardType.Number,
            ),
            modifier = modifier,
        )
        FieldKind.TEXT -> OutlinedTextField(
            value = field.value,
            onValueChange = onChange,
            label = { Text(field.label) },
            singleLine = true,
            modifier = modifier,
        )
    }
}
