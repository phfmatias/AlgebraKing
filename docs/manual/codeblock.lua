-- AlgebraKing user guide: pandoc filter shared by the PDF and HTML builds

-- code blocks as framed grey boxes (MoleKing style) in LaTeX
function CodeBlock(el)
  if FORMAT:match('latex') then
    return pandoc.RawBlock('latex',
      '\\begin{akcode}\n\\begin{Verbatim}[breaklines,breakanywhere,fontsize=\\small]\n'
      .. el.text .. '\n\\end{Verbatim}\n\\end{akcode}')
  end
end

-- drop the hand-written "7." from section titles (LaTeX/pandoc number them);
-- the id was already computed from the full title, so the markdown links still work
function Header(el)
  local first = el.content[1]
  if first and first.t == 'Str' and first.text:match('^%d+[%.%d]*%.?$') then
    el.content:remove(1)
    if el.content[1] and el.content[1].t == 'Space' then
      el.content:remove(1)
    end
  end
  return el
end
