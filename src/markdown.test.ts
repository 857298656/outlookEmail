import { describe, expect, it } from "vitest";
import { api } from "./api";

describe("markdown library API", () => {
  it("manages categories, documents, search and category deletion", async () => {
    const suffix = Date.now().toString();
    const category = await api.createMarkdownCategory(`测试分类-${suffix}`);
    const childCategory = await api.createMarkdownCategory(`子文件夹-${suffix}`, category.id);
    expect(childCategory.parent_id).toBe(category.id);

    const document = await api.createMarkdownDocument({
      title: `发布清单-${suffix}`,
      content: "# 发布\n\n- [ ] 构建安装包",
      category_id: childCategory.id
    });

    const searchResults = await api.listMarkdownDocuments(undefined, "安装包");
    expect(searchResults.some((item) => item.id === document.id)).toBe(true);

    const updated = await api.updateMarkdownDocument({
      id: document.id,
      title: document.title,
      content: "# 发布\n\n- [x] 构建安装包",
      category_id: childCategory.id,
      source_path: "C:\\notes\\release.md"
    });
    expect(updated.content).toContain("[x]");
    expect(updated.source_path).toBe("C:\\notes\\release.md");

    await api.deleteMarkdownCategory(category.id);
    const retained = await api.getMarkdownDocument(document.id);
    expect(retained.category_id).toBeNull();

    await api.deleteMarkdownDocument(document.id);
    const documents = await api.listMarkdownDocuments();
    expect(documents.some((item) => item.id === document.id)).toBe(false);
  });
});
