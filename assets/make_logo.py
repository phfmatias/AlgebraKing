##   PYTHON FILE HEADER #
##
##   File:         [make_logo.py]
##
##   Author(s):    ['Pedro H.F Matias']
##   Site(s):      ['https://github.com/phfmatias']
##   Email(s):     ['pedrohfmatias@gmail.com']
##   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
##   Date:         ['27.10.2026']
##   Version:      ['0.0.1']
##   Status:       ['Development']
##   Language:     ['Python']
##   Description:  ['Build the AlgebraKing logo set in the King family style (crown + symbol + Neuropol name).']
##   Usage:        ['python3 make_logo.py [path/to/ConvergKing.png]']

from os import path
from sys import argv, exit
from PIL import Image, ImageDraw, ImageFont, ImageChops, ImageFilter

HERE = path.dirname(path.abspath(__file__))
# assets/crown_mask.png (tracked) caches the crown cut from CROWN_SRC, so the source
# image is only needed the first time. The Neuropol font (NAME_FONT) is still required.
CROWN_SRC = '/home/phfmatias/Desktop/LOGO_LEED/CV_King/ConvergKing.png'
INTEGRAL_FONT = '/usr/share/fonts/opentype/stix/STIXIntegralsSm-Bold.otf'
NAME_FONT = path.expanduser('~/.fonts/Neuropol.otf')

# King family palettes (sampled from MoleKing/ConvergKing)
PALETTES = {
    'Gold':   {'bg': ('#2A2A2A', '#161616'), 'fg': ('#FAE357', '#B1851C'), 'name': ('#C29A2A', '#A87D18')},
    'Grad':   {'bg': ('#418587', '#0B4D6A'), 'fg': ('#FFFFFF', '#FFFFFF'), 'name': ('#FFFFFF', '#FFFFFF')},
    'Teal':   {'bg': ('#FFFFFF', '#FFFFFF'), 'fg': ('#4E8E8A', '#03445F'), 'name': ('#4E8E8A', '#03445F')},
    'Branco': {'bg': None,                   'fg': ('#FFFFFF', '#FFFFFF'), 'name': ('#FFFFFF', '#FFFFFF')},
}


class LogoMaker():
    def __init__(self, crownSrc):
        self.crownSrc = crownSrc
        self.loadCrown()
        self.makeIntegral()
        self.makeName()
        for key in PALETTES:
            self.buildFull(key)
        self.buildIcon('Gold')
        self.buildIcon('Grad')
        self.buildBanner('Gold')
        print('Logos generated in {}!'.format(HERE))

    def loadCrown(self):
        cache = path.join(HERE, 'crown_mask.png')
        if path.isfile(cache):
            self.crown = Image.open(cache).convert('L')
            return
        if not path.isfile(self.crownSrc):
            print('Error: crown source not found: {}'.format(self.crownSrc))
            exit(1)
        # ConvergKing.png has a transparent background: alpha is the crown mask
        alpha = Image.open(self.crownSrc).convert('RGBA').getchannel('A')
        # crown region of ConvergKing.png (2616x2048), above the wordmark
        mask = alpha.crop((739, 497, 2008, 1507))
        self.crown = mask.crop(mask.getbbox())
        self.crown.save(cache)

    def makeIntegral(self):
        ft = ImageFont.truetype(INTEGRAL_FONT, 1400)
        tmp = Image.new('L', (1800, 2200), 0)
        ImageDraw.Draw(tmp).text((300, 200), '∫', font=ft, fill=255)
        self.integral = tmp.crop(tmp.getbbox())

    def makeName(self):
        ft = ImageFont.truetype(NAME_FONT, 400)
        tmp = Image.new('L', (5200, 800), 0)
        ImageDraw.Draw(tmp).text((100, 100), 'AlgebraKing', font=ft, fill=255)
        self.name = tmp.crop(tmp.getbbox())

    @staticmethod
    def scaled(mask, width=None, height=None):
        w, h = mask.size
        if width:
            return mask.resize((width, round(h * width / w)), Image.LANCZOS)
        return mask.resize((round(w * height / h), height), Image.LANCZOS)

    @staticmethod
    def gradient(size, colors, start=0, end=None):
        # vertical gradient between colors[0] at y=start and colors[1] at y=end
        w, h = size
        end = h if end is None else end
        c0 = ImageColor(colors[0])
        c1 = ImageColor(colors[1])
        col = Image.new('RGB', (1, h))
        for y in range(h):
            t = min(1.0, max(0.0, (y - start) / max(1, end - start)))
            col.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(c0, c1)))
        return col.resize((w, h))

    def background(self, size, pal, radius=0):
        if pal['bg'] is None:
            return Image.new('RGBA', size, (0, 0, 0, 0))
        bg = self.gradient(size, pal['bg']).convert('RGBA')
        if radius:
            round_mask = Image.new('L', size, 0)
            ImageDraw.Draw(round_mask).rounded_rectangle((0, 0, size[0] - 1, size[1] - 1), radius, fill=255)
            bg.putalpha(round_mask)
        return bg

    def paint(self, canvas, mask, xy, colors, gspan=None):
        # paint mask at xy with a vertical gradient (gspan = (y0, y1) on canvas)
        y0, y1 = gspan if gspan else (xy[1], xy[1] + mask.size[1])
        grad = self.gradient(canvas.size, colors, y0, y1).convert('RGBA')
        layer = Image.new('L', canvas.size, 0)
        layer.paste(mask, xy)
        grad.putalpha(layer)
        canvas.alpha_composite(grad)

    def symbol(self, crownWidth):
        # crown sitting over the integral, returned as one mask
        crown = self.scaled(self.crown, width=crownWidth)
        integ = self.scaled(self.integral, height=round(crownWidth * 1.12))
        gap = round(crownWidth * 0.05)
        # the crown sits on the head of the integral: centre of its top 20%
        head = integ.crop((0, 0, integ.size[0], round(integ.size[1] * 0.2))).getbbox()
        headX = (head[0] + head[2]) // 2
        cx = crown.size[0] // 2
        left = max(cx, headX)
        w = left + max(crown.size[0] - cx, integ.size[0] - headX)
        h = crown.size[1] + gap + integ.size[1]
        sym = Image.new('L', (w, h), 0)
        sym.paste(crown, (left - cx, 0))
        sym = ImageChops.lighter(sym, self.place(integ, (w, h), (left - headX, crown.size[1] + gap)))
        return sym

    @staticmethod
    def place(mask, size, xy):
        out = Image.new('L', size, 0)
        out.paste(mask, xy)
        return out

    def buildFull(self, key):
        pal = PALETTES[key]
        sym = self.symbol(900)
        name = self.scaled(self.name, width=1720)
        top = 110
        ny = top + sym.size[1] + 120
        W, H = 2000, ny + name.size[1] + 130
        canvas = self.background((W, H), pal)
        self.paint(canvas, sym, ((W - sym.size[0]) // 2, top), pal['fg'])
        self.paint(canvas, name, ((W - name.size[0]) // 2, ny), pal['name'])
        suffix = '' if key == 'Teal' else '_' + key
        canvas.save(path.join(HERE, 'AlgebraKing{}.png'.format(suffix)))

    def buildIcon(self, key):
        pal = PALETTES[key]
        S = 1024
        canvas = self.background((S, S), pal, radius=200)
        sym = self.symbol(560)
        sym = self.scaled(sym, height=820) if sym.size[1] > 820 else sym
        self.paint(canvas, sym, ((S - sym.size[0]) // 2, (S - sym.size[1]) // 2), pal['fg'])
        canvas.save(path.join(HERE, 'icon_{}.png'.format(key)))

    def buildBanner(self, key):
        pal = PALETTES[key]
        W, H = 2560, 640
        canvas = self.background((W, H), pal, radius=96)
        sym = self.scaled(self.symbol(500), height=520)
        self.paint(canvas, sym, (150, (H - sym.size[1]) // 2), pal['fg'])
        name = self.scaled(self.name, width=1700)
        self.paint(canvas, name, (150 + sym.size[0] + 160, (H - name.size[1]) // 2), pal['name'])
        canvas.save(path.join(HERE, 'banner_{}.png'.format(key)))


def ImageColor(hexColor):
    hexColor = hexColor.lstrip('#')
    return tuple(int(hexColor[i:i + 2], 16) for i in (0, 2, 4))


if __name__ == '__main__':
    LogoMaker(argv[1] if len(argv) > 1 else CROWN_SRC)
