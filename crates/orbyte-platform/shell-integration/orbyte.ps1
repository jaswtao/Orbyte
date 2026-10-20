# Author: jaswtao · Date: 2026-10-14
$global:OrbyteOriginalPrompt = $function:prompt
$global:OrbytePromptSeen = $false
function global:prompt {
    # 在任何其他命令改变 $? 之前保存本次命令结果。
    $succeeded = $?
    $exitCode = if ($succeeded) { 0 } elseif ($null -ne $LASTEXITCODE) { $LASTEXITCODE } else { 1 }
    $esc = [char]27
    $bel = [char]7
    $prefix = ''
    if ($global:OrbytePromptSeen) { $prefix += "${esc}]133;D;$exitCode${bel}" }
    $global:OrbytePromptSeen = $true
    if ($PWD.Provider.Name -eq 'FileSystem') { $prefix += "${esc}]9;9;$($PWD.ProviderPath)${bel}" }
    # 标记嵌入返回值，PSReadLine 重绘提示符时仍能拿到原始文本。
    $text = & $global:OrbyteOriginalPrompt
    return "$prefix${esc}]133;A${bel}$text${esc}]133;B${bel}"
}
