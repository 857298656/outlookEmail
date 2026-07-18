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

    const moved = await api.updateMarkdownDocument({
      id: document.id,
      title: updated.title,
      content: updated.content,
      category_id: category.id,
      source_path: updated.source_path
    });
    expect(moved.category_id).toBe(category.id);
    expect(moved.category_name).toBe(category.name);

    await expect(api.deleteMarkdownCategory(category.id)).rejects.toThrow(
      "文件夹中有子文件或子文件夹"
    );
    const retained = await api.getMarkdownDocument(document.id);
    expect(retained.category_id).toBe(category.id);
    await api.deleteMarkdownDocument(document.id);
    await api.deleteMarkdownCategory(childCategory.id);
    await api.deleteMarkdownCategory(category.id);
    const documents = await api.listMarkdownDocuments();
    expect(documents.some((item) => item.id === document.id)).toBe(false);
  });
});
