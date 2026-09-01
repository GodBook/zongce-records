import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("应用主流程", () => {
  it("加载本地记录并通过 Ctrl+N 打开新增抽屉", async () => {
    render(<App />);

    await screen.findByText("国家级大学生创新训练项目结题");
    expect(
      screen.getByRole("heading", { name: "综测记录" }),
    ).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(await screen.findByText("新增综测记录")).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText("例如：全国大学生数学建模竞赛二等奖"),
    ).toBeInTheDocument();
  });

  it("导航到设置后显示类别、备份和更新入口", async () => {
    render(<App />);
    await screen.findByText("国家级大学生创新训练项目结题");

    fireEvent.click(screen.getByRole("button", { name: "设置" }));

    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "设置" })).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByRole("tab", { name: "活动类别" }));
    expect(
      screen.getByRole("heading", { name: "活动类别" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "备份与存储" }));
    expect(
      screen.getByRole("button", { name: "创建备份" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "软件更新" }));
    expect(
      screen.getByRole("button", { name: "检查更新" }),
    ).toBeInTheDocument();
  });
});
