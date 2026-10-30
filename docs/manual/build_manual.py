##   PYTHON FILE HEADER #
##
##   File:         [build_manual.py]
##
##   Author(s):    ['Pedro H.F Matias']
##   Site(s):      ['https://github.com/phfmatias']
##   Email(s):     ['pedrohfmatias@gmail.com']
##   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
##   Date:         ['30.10.2026']
##   Version:      ['0.0.2']
##   Status:       ['Development']
##   Language:     ['Python']
##   Description:  ['Build the user guide in PT-BR and EN-US as PDF (LaTeX, MoleKing style) and HTML.']
##   Usage:        ['python3 docs/manual/build_manual.py [pt|en]']

from os import path, remove
from sys import argv, exit
from subprocess import run
from shutil import move

HERE = path.dirname(path.abspath(__file__))
DOCS = path.dirname(HERE)
ROOT = path.dirname(DOCS)
LOGO = path.join(ROOT, 'assets', 'AlgebraKing_Gold.png')
REPO = 'https://github.com/phfmatias/AlgebraKing'

LANGS = {
    'pt': {'source': 'GUIA_USUARIO.md', 'out': 'GUIA_USUARIO', 'toc': 'Sumário', 'babel': 'brazil', 'lang': 'pt-BR',
           'title': 'AlgebraKing: Um Sistema de Álgebra Computacional para o Terminal e para o Claude.',
           'subtitle': 'Guia do Usuário, Versão {}',
           'institution': 'Universidade Federal de Goiás',
           'lab': 'LEEDMOL - Laboratório de Estrutura Eletrônica e Dinâmica Molecular.',
           'header': 'AlgebraKing: Guia do Usuário.'},
    'en': {'source': 'USER_GUIDE.md', 'out': 'USER_GUIDE', 'toc': 'Contents', 'babel': 'english', 'lang': 'en-US',
           'title': 'AlgebraKing: A Computer Algebra System for the Terminal and for Claude.',
           'subtitle': 'User Guide, Version {}',
           'institution': 'Universidade Federal de Goiás',
           'lab': 'LEEDMOL - Laboratory of Electronic Structure and Molecular Dynamics.',
           'header': 'AlgebraKing: User Guide.'},
}


def latexEscape(text):
    for a, b in (('\\', r'\textbackslash{}'), ('&', r'\&'), ('%', r'\%'), ('#', r'\#'), ('_', r'\_')):
        text = text.replace(a, b)
    return text


class ManualBuilder():
    def __init__(self, key):
        self.key = key
        self.conf = LANGS[key]
        self.source = path.join(DOCS, self.conf['source'])
        if not path.isfile(self.source):
            print('Error: {} not found.'.format(self.source))
            exit(1)
        self.getVersion()
        self.prepareSource()
        self.makePDF()
        self.makeHTML()
        self.cleanUp()
        print('{}: docs/{}.pdf and docs/{}.html generated!'.format(key, self.conf['out'], self.conf['out']))

    def getVersion(self):
        self.version = '?'
        with open(path.join(ROOT, 'Cargo.toml'), 'r') as f:
            for line in f:
                if line.split('=')[0].strip() == 'version':
                    self.version = line.split('=')[1].strip().strip('"')
                    break

    def prepareSource(self):
        # drop the title and the hand-written contents block (pandoc builds its own)
        with open(self.source, 'r') as f:
            rlines = f.readlines()
        start, end = None, None
        for i in range(len(rlines)):
            if start is None and rlines[i].startswith('## ') and rlines[i].strip()[3:] == self.conf['toc']:
                start = i
            elif start is not None and rlines[i].strip() == '---':
                end = i + 1
                break
        if start is None or end is None:
            print('Error: "## {}" block not found in {}'.format(self.conf['toc'], self.source))
            exit(1)
        body = [x for x in rlines[:start] if not x.startswith('# ')] + rlines[end:]
        out = body
        self.tmpSource = path.join(HERE, '_guide_{}.md'.format(self.key))
        with open(self.tmpSource, 'w') as f:
            f.writelines(out)

    def pandocVars(self, escape):
        conv = latexEscape if escape else (lambda x: x)
        return {'title': conv(self.conf['title']),
                'subtitle': conv(self.conf['subtitle'].format(self.version)),
                'institution': conv(self.conf['institution']),
                'lab': conv(self.conf['lab']),
                'header': conv(self.conf['header'])}

    def makePDF(self):
        tex = path.join(HERE, '_{}.tex'.format(self.key))
        cmd = ['pandoc', self.tmpSource, '-f', 'markdown+gfm_auto_identifiers-implicit_figures',
               '-t', 'latex', '--template', path.join(HERE, 'template.tex'),
               '--lua-filter', path.join(HERE, 'codeblock.lua'),
               '--number-sections', '--top-level-division=section', '--shift-heading-level-by=-1',
               '-V', 'babel=' + self.conf['babel'], '-V', 'logo=' + LOGO,
               '-V', 'repo=\\url{' + REPO + '}', '-o', tex]
        for k, v in self.pandocVars(True).items():
            cmd += ['-V', '{}={}'.format(k, v)]
        result = run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            print('Error: pandoc (LaTeX) failed.\n{}'.format(result.stderr))
            exit(1)
        # two passes for the table of contents
        for _ in range(2):
            result = run(['lualatex', '-interaction=nonstopmode', '-halt-on-error', path.basename(tex)],
                         cwd=HERE, capture_output=True, text=True)
            if result.returncode != 0:
                print('Error: lualatex failed, see {}'.format(tex.replace('.tex', '.log')))
                print('\n'.join(result.stdout.splitlines()[-25:]))
                exit(1)
        move(tex.replace('.tex', '.pdf'), path.join(DOCS, self.conf['out'] + '.pdf'))
        self.texFiles = [tex.replace('.tex', ext) for ext in ('.tex', '.aux', '.log', '.toc', '.out')]

    def makeHTML(self):
        other = 'en' if self.key == 'pt' else 'pt'
        v = self.pandocVars(False)
        write = '<div class="langbar">'
        for k, label in (('pt', 'Português'), ('en', 'English')):
            if k == self.key:
                write += '<span class="current">{}</span>'.format(label)
            else:
                write += '<a href="{}.html">{}</a>'.format(LANGS[k]['out'], label)
            write += ' · ' if k == 'pt' else ''
        write += '</div>\n<header class="titleblock">\n<hr>\n'
        write += '<div class="title">{}</div>\n<hr>\n<div class="subtitle">{}</div>\n<hr>\n'.format(v['title'], v['subtitle'])
        write += '<div class="inst">{}</div>\n<div>{}</div>\n'.format(v['institution'], v['lab'])
        write += '<img src="{}" alt="AlgebraKing">\n'.format(path.relpath(LOGO, HERE))
        write += '<div class="repo"><a href="{0}">{0}</a></div>\n</header>\n'.format(REPO)
        write += '<style>nav#TOC{{--toc-title:"{}";}}</style>\n'.format(self.conf['toc'])
        cover = path.join(HERE, '_cover_{}.html'.format(self.key))
        with open(cover, 'w') as f:
            f.write(write)
        cmd = ['pandoc', self.tmpSource, '-f', 'markdown+gfm_auto_identifiers-implicit_figures',
               '-t', 'html5', '-s', '--self-contained', '--toc', '--toc-depth=3',
               '--number-sections', '--shift-heading-level-by=-1', '--no-highlight',
               '--lua-filter', path.join(HERE, 'codeblock.lua'),
               '--metadata', 'pagetitle=AlgebraKing — ' + v['subtitle'],
               '--metadata', 'lang=' + self.conf['lang'],
               '--resource-path', HERE, '-c', path.join(HERE, 'guide.css'), '-B', cover,
               '-o', path.join(DOCS, self.conf['out'] + '.html')]
        result = run(cmd, cwd=HERE, capture_output=True, text=True)
        if result.returncode != 0:
            print('Error: pandoc (HTML) failed.\n{}'.format(result.stderr))
            exit(1)
        self.cover = cover
        self.other = other

    def cleanUp(self):
        for arq in self.texFiles + [self.tmpSource, self.cover]:
            if path.isfile(arq):
                remove(arq)


if __name__ == '__main__':
    keys = argv[1:] if len(argv) > 1 else list(LANGS)
    for key in keys:
        if key not in LANGS:
            print('Usage: python3 docs/manual/build_manual.py [pt|en]')
            exit(1)
        ManualBuilder(key)
