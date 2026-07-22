import MarkdownIt from "markdown-it";
import MdContainer from "markdown-it-container";
import MdDiv from "markdown-it-div";
import * as MdEmoji from "markdown-it-emoji";
import MdHContainer from "./h-container";
import MdNContainer from "./n-container";
import { colorPlugin } from "./color-plugin";
import svgMap from "./svg-map";

const markdownParserResume = new MarkdownIt({
    html: true,
    breaks: true,
});

markdownParserResume
    .use(MdEmoji.full, {
        defs: svgMap,
        shortcuts: Object.keys(svgMap).reduce<Record<string, string>>((obj, key) => {
            obj[key] = `icon:${key}`;
            return obj;
        }, {}),
    })
    .use(MdHContainer)
    .use(MdContainer, "header")
    .use(MdContainer, "left", {
        render: function (tokens: any, idx: any) {
            if (tokens[idx].nesting === 1) {
                return '<div class="lr-container"><div class="left">';
            } else {
                return "</div>";
            }
        },
    })
    .use(MdContainer, "right", {
        render: function (tokens: any, idx: any) {
            if (tokens[idx].nesting === 1) {
                return '<div class="right">';
            } else {
                return "</div></div>";
            }
        },
    })
    .use(MdContainer, "title")
    .use(MdNContainer)
    .use(MdDiv);

export function renderResumeMarkdown(content: string, color: string): string {
    const rawHtml = markdownParserResume.render(content);
    return colorPlugin(rawHtml, color);
}

export { markdownParserResume };
