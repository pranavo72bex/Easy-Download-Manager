// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.ui

import android.content.Context
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import com.dm.android.R
import uniffi.dm_android.RowState

/** Material 3, in the phone's own colours where it has them (Android 12 and up). */
@Composable
fun DmTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val colors = when {
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> darkColorScheme()
        else -> lightColorScheme()
    }
    MaterialTheme(colorScheme = colors, content = content)
}

/** A state's word. */
@Composable
fun stateWord(state: RowState): String = stringResource(
    when (state) {
        RowState.QUEUED -> R.string.state_queued
        RowState.RUNNING -> R.string.state_running
        RowState.PAUSED -> R.string.state_paused
        RowState.COMPLETED -> R.string.state_completed
        RowState.FAILED -> R.string.state_failed
        RowState.CANCELLED -> R.string.state_cancelled
    },
)

/** A message's words, outside composition (the snackbar's collector). */
fun UiMessage.words(context: Context): String = when (this) {
    is UiMessage.Text -> text
    is UiMessage.Res -> if (arg == null) context.getString(id) else context.getString(id, arg)
    is UiMessage.Plural -> context.resources.getQuantityString(id, count, count)
}

/** A message's words, in composition. */
@Composable
fun UiMessage.words(): String = when (this) {
    is UiMessage.Text -> text
    is UiMessage.Res -> if (arg == null) stringResource(id) else stringResource(id, arg)
    is UiMessage.Plural -> LocalContext.current.resources.getQuantityString(id, count, count)
}
