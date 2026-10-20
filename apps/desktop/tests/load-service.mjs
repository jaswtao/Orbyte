/** 纯 TS 模块测试加载器。Author: jaswtao · Date: 2026-10-14 */
import { readFile } from 'node:fs/promises'
import ts from 'typescript'

export async function loadService(path) {
  const url = new URL(`../src/services/${path}.ts`, import.meta.url)
  const source = await bundle(url, new Set(), true)
  const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } })
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
}

async function bundle(url, seen, isEntry) {
  if (seen.has(url.href)) return ''
  seen.add(url.href)
  let source = (await readFile(url, 'utf8')).replace(/\r\n/g, '\n')
  source = source.replace(/^\s*import\s+type\b[^\n]*$/gm, '')
  source = source.replace(/import\s+\{([^}]+)\}\s+from/g, (full, names) => {
    const kept = names.split(',').map((name) => name.trim()).filter((name) => name && !name.startsWith('type '))
    return kept.length ? `import { ${kept.join(', ')} } from` : ''
  })
  let prelude = ''
  const importRe = /^[ \t]*import\s+(?:(\w+)|\{([^}]+)\})\s+from\s+'([^']+)'\s*;?[ \t]*$/gm
  const imports = [...source.matchAll(importRe)]
  source = source.replace(importRe, '')
  for (const match of imports) {
    const [, defaultName, named, spec] = match
    if (spec.endsWith('.json')) {
      const value = JSON.parse(await readFile(new URL(spec, url), 'utf8'))
      prelude += `const ${defaultName} = ${JSON.stringify(value)}\n`
    } else if (spec.startsWith('.')) {
      const dep = new URL(spec.endsWith('.ts') ? spec : `${spec}.ts`, url)
      prelude += await bundle(dep, seen, false)
    } else {
      prelude += match[0] + '\n'
    }
    if (named && !spec.startsWith('.') && !spec.endsWith('.json')) {
      // named external imports are kept as the original statement above
    }
  }
  if (!isEntry) {
    source = source.replace(/^export\s+type\s+\{[^}]*\}\s*;?\s*$/gm, '')
    source = source.replace(/^export\s+(?=type\b)/gm, '')
    source = source.replace(/^export\s+(?=(async\s+)?(function|const|class|let|var)\b)/gm, '')
    source = source.replace(/^export\s+\{[^}]*\}\s*;?\s*$/gm, '')
  }
  return `${prelude}\n${source}\n`
}
