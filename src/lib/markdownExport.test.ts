import { describe, expect, it } from "vitest";
import {
  cleanMarkdownEditorHtml,
  standaloneMarkdownHtml
} from "./markdownExport";

describe("Markdown export cleanup", () => {
  it("removes editor controls and converts CodeMirror blocks to semantic code", () => {
    const rendered = document.createElement("div");
    rendered.innerHTML = `
      <h1 contenteditable="true" data-editor-node="heading">发布说明</h1>
      <div class="markdownTableToolbar"><button>新增一行</button></div>
      <div class="markdownImageSourceEditor"><input value="![image.png](attachment:test)"></div>
      <p class="crepe-placeholder" data-placeholder="Start writing..."><br></p>
      <div class="milkdown-code-block">
        <div class="tools"><button>复制</button></div>
        <div class="cm-content">
          <div class="cm-line">const answer = 42;</div>
          <div class="cm-line">console.log(answer);</div>
        </div>
      </div>
      <a href="javascript:alert(1)" onclick="alert(2)">不安全链接</a>
      <a href="https://example.com/docs">安全链接</a>
      <img src="data:image/png;base64,AA==" aria-label="示例图片">
      <script>alert("xss")</script>
    `;

    const html = cleanMarkdownEditorHtml(rendered);

    expect(html).toContain("<h1>发布说明</h1>");
    expect(html).toContain("<pre><code>const answer = 42;\nconsole.log(answer);</code></pre>");
    expect(html).toContain('href="https://example.com/docs"');
    expect(html).toContain('src="data:image/png;base64,AA=="');
    expect(html).not.toContain("markdownTableToolbar");
    expect(html).not.toContain("milkdown-code-block");
    expect(html).not.toContain("markdownImageSourceEditor");
    expect(html).not.toContain("attachment:test");
    expect(html).not.toContain("crepe-placeholder");
    expect(html).not.toContain("data-placeholder");
    expect(html).not.toContain("javascript:");
    expect(html).not.toContain("onclick");
    expect(html).not.toContain("<script");
    expect(html).not.toContain("contenteditable");
    expect(html).not.toContain("data-editor-node");
  });

  it("escapes the document title in standalone HTML", () => {
    const html = standaloneMarkdownHtml(`A <script>"title"</script>`, "<p>正文</p>");
    expect(html).toContain("<title>A &lt;script&gt;&quot;title&quot;&lt;/script&gt;</title>");
    expect(html).toContain("<body><p>正文</p></body>");
  });
});
