#!/usr/bin/env node
/**
 * Generate MathJax reference PNGs for prooftree golden test comparison.
 *
 * KaTeX does not support bussproofs/prooftree, so MathJax (which supports it
 * via the bussproofs extension) is used as the reference renderer.
 *
 * Usage:
 *   node generate_reference_prooftree.mjs [test_cases.txt] [fixtures_dir]
 */

import { readFileSync, writeFileSync, unlinkSync, mkdirSync, existsSync } from 'fs';
import { dirname, isAbsolute, join, resolve } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';
import puppeteer from 'puppeteer';

const __dirname = dirname(fileURLToPath(import.meta.url));

const TEST_CASES = process.argv[2]
  ? (isAbsolute(process.argv[2]) ? process.argv[2] : resolve(process.cwd(), process.argv[2]))
  : join(__dirname, '..', '..', 'tests', 'golden', 'test_cases_prooftree.txt');

const FIXTURES_DIR = process.argv[3]
  ? (isAbsolute(process.argv[3]) ? process.argv[3] : resolve(process.cwd(), process.argv[3]))
  : join(__dirname, '..', '..', 'tests', 'golden', 'fixtures_prooftree');

const LOCAL_MATHJAX_SCRIPT = join(
    __dirname,
    '..',
    'bench-mj-ratex',
    'node_modules',
    'mathjax',
    'es5',
    'tex-svg-full.js',
);
const MATHJAX_SCRIPT = existsSync(LOCAL_MATHJAX_SCRIPT)
    ? pathToFileURL(LOCAL_MATHJAX_SCRIPT).href
    : 'https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-svg-full.js';

function normalizeForMathJax(tex) {
    // MathJax bussproofs requires \fCenter in math mode; convert to an inline
    // math relation so prooftree cases like "A \fCenter B" render correctly.
    return tex.replace(/\\fCenter\b/g, '$\\Rightarrow$');
}

function readPngSize(absPath) {
    const buf = readFileSync(absPath);
    const w = buf.readUInt32BE(16);
    const h = buf.readUInt32BE(20);
    return { width: w, height: h };
}

const lines = readFileSync(TEST_CASES, 'utf-8')
    .split('\n')
    .map(l => l.trim())
    .filter(l => l && !l.startsWith('#'));

if (lines.length === 0) {
    console.error('No formulas found in', TEST_CASES);
    process.exit(1);
}

mkdirSync(FIXTURES_DIR, { recursive: true });

const TEMP_HTML = join(FIXTURES_DIR, '_render.html');

const HTML = `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<script>
MathJax = {
  loader: {
    // Explicitly load bussproofs so \\begin{prooftree} is recognized.
    load: ['[tex]/bussproofs'],
  },
  tex: {
    packages: { '[+]': ['bussproofs'] },
    inlineMath: [['$','$']],
    displayMath: [['$$','$$']],
    processEscapes: false,
  },
  svg: { fontCache: 'none' },
  options: {
    enableMenu: false,
  },
};
</script>
<script id="MathJax-script" src="${MATHJAX_SCRIPT}"></script>
<style>
  body { margin: 0; background: white; }
  #formula {
    display: inline-block;
    padding: 24px;
    font-size: 36px;
    line-height: 1.2;
  }
  mjx-container[display="true"] {
    margin: 0 !important;
    text-align: left !important;
  }
  .MathJax { outline: none; }
</style>
</head>
<body><div id="formula"></div></body>
</html>`;

async function main() {
    writeFileSync(TEMP_HTML, HTML);

    const browser = await puppeteer.launch({
        headless: 'new',
        args: ['--no-sandbox', '--disable-setuid-sandbox'],
    });

    let ok = 0;
    let errors = 0;

    for (let i = 0; i < lines.length; i++) {
        const expr = lines[i];
        const idx = String(i + 1).padStart(4, '0');
        const outPath = join(FIXTURES_DIR, `${idx}.png`);
        const page = await browser.newPage();

        try {
            await page.setViewport({ width: 1200, height: 600, deviceScaleFactor: 1 });
            await page.goto('file://' + TEMP_HTML, { waitUntil: 'networkidle0' });

            // Strip outer $...$ if present
            let toRender = expr;
            const outer = toRender.match(/^\$(.*)\$$/s);
            if (outer) toRender = outer[1];

            // Normalize known MathJax bussproofs incompatibilities first.
            const normalized = normalizeForMathJax(toRender);

            // Set the formula as display math and wait for MathJax typesetting
            await page.evaluate(async (tex) => {
                const el = document.getElementById('formula');
                el.innerHTML = '$$' + tex + '$$';
                await MathJax.typesetPromise([el]);
            }, normalized);

            // Detect MathJax TeX parse errors (including noerrors fallback output).
            const mjError = await page.evaluate(() => {
                const merror = document.querySelector('#formula mjx-merror');
                if (merror) {
                    const text = merror.getAttribute('data-mjx-error') || merror.textContent || 'MathJax error';
                    return text.trim();
                }
                const errNode = document.querySelector('#formula [data-mjx-error]');
                if (!errNode) return null;
                const text = errNode.getAttribute('data-mjx-error') || errNode.textContent || 'MathJax error';
                return text.trim();
            });
            if (mjError) {
                throw new Error(mjError);
            }

            // Verify SVG output exists
            await page.waitForFunction(() => {
                return document.getElementById('formula').querySelector('svg') !== null;
            }, { timeout: 10000 });

            const el = await page.$('#formula');
            if (!el) {
                console.error(`ERR  ${idx}: ${expr} — no formula element`);
                errors++;
                await page.close();
                continue;
            }

            // MathJax bussproofs labels may extend outside #formula's own box.
            // Use the union bounds of #formula and rendered SVG containers only.
            // Avoid including assistive MathML descendants, which can inflate bounds.
            const clip = await page.evaluate(() => {
                const root = document.getElementById('formula');
                if (!root) return null;

                const nodes = [
                    root,
                    ...root.querySelectorAll('mjx-container[jax=\"SVG\"]'),
                    ...root.querySelectorAll('mjx-container[jax=\"SVG\"] svg'),
                    ...root.querySelectorAll('mjx-container[jax=\"SVG\"] svg *'),
                ];
                let minX = Number.POSITIVE_INFINITY;
                let minY = Number.POSITIVE_INFINITY;
                let maxX = Number.NEGATIVE_INFINITY;
                let maxY = Number.NEGATIVE_INFINITY;

                for (const node of nodes) {
                    const rect = node.getBoundingClientRect();
                    if (rect.width <= 0 || rect.height <= 0) continue;
                    minX = Math.min(minX, rect.left);
                    minY = Math.min(minY, rect.top);
                    maxX = Math.max(maxX, rect.right);
                    maxY = Math.max(maxY, rect.bottom);
                }

                if (!Number.isFinite(minX)) return null;
                const pad = 2;
                return {
                    x: Math.max(0, minX - pad),
                    y: Math.max(0, minY - pad),
                    width: Math.max(1, maxX - minX + pad * 2),
                    height: Math.max(1, maxY - minY + pad * 2),
                };
            });

            if (!clip || clip.width === 0 || clip.height === 0) {
                console.error(`ERR  ${idx}: ${expr} — empty clip region`);
                errors++;
                await page.close();
                continue;
            }

            // Screenshot formula using union bounds to include overflowed labels.
            await page.screenshot({
                path: outPath,
                type: 'png',
                omitBackground: true,
                clip,
            });

            // Trim transparent edges (MathJax adds padding)
            try {
                const size = readPngSize(outPath);
                console.log(`OK  ${idx} → ${outPath} (${size.width}x${size.height})`);
            } catch {
                console.log(`OK  ${idx} → ${outPath}`);
            }
            ok++;
        } catch (err) {
            console.error(`ERR  ${idx}: ${expr} — ${err.message}`);
            errors++;
        } finally {
            await page.close();
        }
    }

    await browser.close();
    try { unlinkSync(TEMP_HTML); } catch (_) {}
    console.log(`\nDone: ${ok} OK, ${errors} errors out of ${lines.length} formulas`);
}

main().catch(err => { console.error(err); process.exit(1); });
