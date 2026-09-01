import { useEffect, useMemo, useRef, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  DrawerBody,
  DrawerFooter,
  DrawerHeader,
  DrawerHeaderTitle,
  Field,
  Input,
  OverlayDrawer,
  Select,
  Textarea,
  Tooltip,
} from "@fluentui/react-components";
import { zodResolver } from "@hookform/resolvers/zod";
import {
  ExternalLink,
  Eye,
  FilePlus2,
  FileText,
  FolderOpen,
  Image as ImageIcon,
  Paperclip,
  Save,
  Trash2,
  X,
} from "lucide-react";
import { Controller, useForm } from "react-hook-form";
import { z } from "zod";
import type {
  AssessmentLevel,
  AssessmentRecord,
  Category,
  Material,
  PendingMaterial,
  MaterialPreview,
  RecordDraft,
} from "../types";
import { api } from "../lib/api";
import {
  LEVEL_META,
  createId,
  errorMessage,
  formatBytes,
  scoreToCents,
  todayLocal,
} from "../lib/utils";
import { ConfirmDialog } from "./Common";

const iconProps = { size: 18, strokeWidth: 1.8 } as const;
const MAX_FILES = 20;

const recordSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, "请输入活动名称")
    .max(200, "活动名称不能超过 200 个字"),
  categoryId: z.string().min(1, "请选择活动类别"),
  level: z.enum(["college", "school", "provincial", "national"]),
  date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "请选择活动日期"),
  score: z
    .string()
    .trim()
    .min(1, "请输入分数")
    .regex(/^\d+(?:\.\d{1,2})?$/, "请输入非负数，最多保留两位小数"),
  notes: z.string().max(2000, "备注不能超过 2000 个字"),
});

type RecordFormValues = z.infer<typeof recordSchema>;

function defaults(
  record?: AssessmentRecord | null,
  categories?: Category[],
): RecordFormValues {
  return {
    name: record?.name ?? "",
    categoryId:
      record?.categoryId ??
      categories?.find((category) => category.isActive)?.id ??
      "",
    level: record?.level ?? "college",
    date: record?.date ?? todayLocal(),
    score: record?.score ?? "",
    notes: record?.notes ?? "",
  };
}

function MaterialIcon({ mimeType }: { mimeType: string }) {
  if (mimeType.startsWith("image/")) return <ImageIcon {...iconProps} />;
  if (mimeType === "application/pdf") return <FileText {...iconProps} />;
  return <Paperclip {...iconProps} />;
}

export function RecordDrawer({
  open,
  record,
  categories,
  onClose,
  onSaved,
  notify,
}: {
  open: boolean;
  record: AssessmentRecord | null;
  categories: Category[];
  onClose: () => void;
  onSaved: (record: AssessmentRecord) => void;
  notify: (
    kind: "success" | "error" | "info",
    title: string,
    message?: string,
  ) => void;
}) {
  const [existingMaterials, setExistingMaterials] = useState<Material[]>([]);
  const [pendingMaterials, setPendingMaterials] = useState<PendingMaterial[]>(
    [],
  );
  const [materialsDirty, setMaterialsDirty] = useState(false);
  const [materialError, setMaterialError] = useState("");
  const [previewOpen, setPreviewOpen] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [preview, setPreview] = useState<MaterialPreview | null>(null);
  const [previewMaterialId, setPreviewMaterialId] = useState("");
  const [saving, setSaving] = useState(false);
  const [confirmHighScore, setConfirmHighScore] = useState(false);
  const pendingSubmit = useRef<RecordFormValues | null>(null);
  const browserFileInput = useRef<HTMLInputElement>(null);
  const form = useForm<RecordFormValues>({
    resolver: zodResolver(recordSchema),
    defaultValues: defaults(record, categories),
    mode: "onBlur",
  });
  const {
    control,
    register,
    reset,
    handleSubmit,
    formState: { errors, isDirty },
    watch,
  } = form;

  useEffect(() => {
    if (!open) return;
    reset(defaults(record, categories));
    setExistingMaterials(record?.materials ?? []);
    setPendingMaterials([]);
    setMaterialsDirty(false);
    setMaterialError("");
    setPreviewOpen(false);
    setPreviewLoading(false);
    setPreview(null);
    setPreviewMaterialId("");
    setConfirmHighScore(false);
  }, [open, record, categories, reset]);

  const hasUnsavedChanges = isDirty || materialsDirty;
  const selectedDate = watch("date");
  const futureDate = selectedDate > todayLocal();
  const activeCategories = useMemo(
    () =>
      categories.filter(
        (category) => category.isActive || category.id === record?.categoryId,
      ),
    [categories, record?.categoryId],
  );

  function requestClose() {
    if (saving) return;
    if (hasUnsavedChanges && !window.confirm("当前修改尚未保存，确定关闭吗？"))
      return;
    onClose();
  }

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        requestClose();
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        document
          .querySelector<HTMLFormElement>("#record-editor-form")
          ?.requestSubmit();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  });

  async function appendMaterials(files?: FileList | File[]) {
    setMaterialError("");
    try {
      const selected = await api.chooseMaterials(files);
      if (
        existingMaterials.length + pendingMaterials.length + selected.length >
        MAX_FILES
      ) {
        setMaterialError(`每条记录最多添加 ${MAX_FILES} 个证明材料`);
        return;
      }
      setPendingMaterials((current) => [...current, ...selected]);
      setMaterialsDirty(true);
    } catch (error) {
      setMaterialError(errorMessage(error));
    } finally {
      if (browserFileInput.current) browserFileInput.current.value = "";
    }
  }

  function chooseMaterials() {
    if (api.isTauri()) void appendMaterials();
    else browserFileInput.current?.click();
  }

  function removeExisting(id: string) {
    setExistingMaterials((current) =>
      current.filter((material) => material.id !== id),
    );
    setMaterialsDirty(true);
  }

  async function showPreview(material: Material) {
    setPreviewOpen(true);
    setPreviewLoading(true);
    setPreview(null);
    setPreviewMaterialId(material.id);
    try {
      const nextPreview = await api.getMaterialPreview(material.id);
      if (!nextPreview) {
        setPreviewOpen(false);
        notify("info", "当前环境无法预览", "请使用系统程序打开证明材料");
        return;
      }
      setPreview(nextPreview);
    } catch (error) {
      setPreviewOpen(false);
      notify("error", "无法预览材料", errorMessage(error));
    } finally {
      setPreviewLoading(false);
    }
  }

  async function openWithSystem(materialId: string) {
    try {
      await api.openMaterial(materialId);
    } catch (error) {
      notify("error", "无法打开材料", errorMessage(error));
    }
  }

  async function persist(values: RecordFormValues) {
    setSaving(true);
    try {
      const draft: RecordDraft = {
        id: record?.id ?? createId(),
        revision: record?.revision ?? 0,
        name: values.name,
        categoryId: values.categoryId,
        level: values.level as AssessmentLevel,
        date: values.date,
        score: values.score,
        notes: values.notes,
        attachmentIds: existingMaterials.map((material) => material.id),
        newAttachments: pendingMaterials,
      };
      const saved = await api.saveRecord(draft);
      reset(defaults(saved, categories));
      setMaterialsDirty(false);
      notify("success", record ? "记录已更新" : "记录已添加", saved.name);
      onSaved(saved);
    } catch (error) {
      notify("error", "保存失败", errorMessage(error));
    } finally {
      setSaving(false);
    }
  }

  function onValid(values: RecordFormValues) {
    if (scoreToCents(values.score) > 100_000) {
      pendingSubmit.current = values;
      setConfirmHighScore(true);
      return;
    }
    void persist(values);
  }

  return (
    <>
      <OverlayDrawer
        open={open}
        position="end"
        size="medium"
        className="record-drawer"
        onOpenChange={(_, data) => !data.open && requestClose()}
      >
        <DrawerHeader>
          <DrawerHeaderTitle
            action={
              <Tooltip content="关闭" relationship="label">
                <Button
                  appearance="subtle"
                  icon={<X {...iconProps} />}
                  aria-label="关闭记录编辑器"
                  onClick={requestClose}
                />
              </Tooltip>
            }
          >
            <span className="drawer-title">
              {record ? "编辑综测记录" : "新增综测记录"}
            </span>
            <span className="drawer-subtitle">
              {record
                ? `最后更新于 ${record.updatedAt.slice(0, 10)}`
                : "填写活动信息并归档证明材料"}
            </span>
          </DrawerHeaderTitle>
        </DrawerHeader>

        <DrawerBody>
          <form
            id="record-editor-form"
            className="record-form"
            onSubmit={handleSubmit(onValid)}
          >
            <Field
              label="活动名称"
              required
              validationState={errors.name ? "error" : "none"}
              validationMessage={errors.name?.message}
            >
              <Controller
                name="name"
                control={control}
                render={({ field }) => (
                  <Input
                    ref={field.ref}
                    name={field.name}
                    value={field.value}
                    onBlur={field.onBlur}
                    onChange={(_, data) => field.onChange(data.value)}
                    autoFocus
                    maxLength={200}
                    placeholder="例如：全国大学生数学建模竞赛二等奖"
                  />
                )}
              />
            </Field>

            <div className="form-grid-two">
              <Field
                label="活动类别"
                required
                validationState={errors.categoryId ? "error" : "none"}
                validationMessage={errors.categoryId?.message}
              >
                <Select {...register("categoryId")}>
                  <option value="">请选择</option>
                  {activeCategories.map((category) => (
                    <option value={category.id} key={category.id}>
                      {category.name}
                      {category.isActive ? "" : "（已停用）"}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="综测级别" required>
                <Select {...register("level")}>
                  {(
                    Object.entries(LEVEL_META) as Array<
                      [AssessmentLevel, (typeof LEVEL_META)[AssessmentLevel]]
                    >
                  ).map(([value, meta]) => (
                    <option value={value} key={value}>
                      {meta.label}
                    </option>
                  ))}
                </Select>
              </Field>
            </div>

            <div className="form-grid-two">
              <Field
                label="活动日期"
                required
                validationState={
                  errors.date ? "error" : futureDate ? "warning" : "none"
                }
                validationMessage={
                  errors.date?.message ??
                  (futureDate ? "日期晚于今天，请确认是否正确" : undefined)
                }
              >
                <Controller
                  name="date"
                  control={control}
                  render={({ field }) => (
                    <Input
                      ref={field.ref}
                      name={field.name}
                      type="date"
                      value={field.value}
                      onBlur={field.onBlur}
                      onChange={(_, data) => field.onChange(data.value)}
                    />
                  )}
                />
              </Field>
              <Field
                label="综测分数"
                required
                hint="最多保留两位小数"
                validationState={errors.score ? "error" : "none"}
                validationMessage={errors.score?.message}
              >
                <Controller
                  name="score"
                  control={control}
                  render={({ field }) => (
                    <Input
                      ref={field.ref}
                      name={field.name}
                      value={field.value}
                      onBlur={field.onBlur}
                      onChange={(_, data) => field.onChange(data.value)}
                      inputMode="decimal"
                      placeholder="0.00"
                      contentAfter={<span className="input-unit">分</span>}
                    />
                  )}
                />
              </Field>
            </div>

            <Field
              label="备注"
              validationState={errors.notes ? "error" : "none"}
              validationMessage={errors.notes?.message}
            >
              <Textarea
                {...register("notes")}
                resize="vertical"
                rows={4}
                maxLength={2000}
                placeholder="可记录获奖等级、承担角色或认定依据"
              />
            </Field>

            <section
              className="materials-section"
              aria-labelledby="materials-title"
            >
              <div className="materials-heading">
                <div>
                  <h3 id="materials-title">证明材料</h3>
                  <p>最多 20 个文件，单个文件不超过 200 MB</p>
                </div>
                <Button
                  type="button"
                  appearance="secondary"
                  icon={<FilePlus2 {...iconProps} />}
                  onClick={chooseMaterials}
                >
                  添加文件
                </Button>
                <input
                  ref={browserFileInput}
                  className="visually-hidden"
                  type="file"
                  multiple
                  aria-label="选择证明材料"
                  onChange={(event) =>
                    void appendMaterials(event.currentTarget.files ?? undefined)
                  }
                />
              </div>
              {materialError ? (
                <p className="field-error" role="alert">
                  {materialError}
                </p>
              ) : null}
              {existingMaterials.length + pendingMaterials.length === 0 ? (
                <button
                  type="button"
                  className="material-dropzone"
                  onClick={chooseMaterials}
                >
                  <Paperclip size={24} strokeWidth={1.6} aria-hidden="true" />
                  <span>暂未添加证明材料</span>
                  <small>可以先保存，之后再补充</small>
                </button>
              ) : (
                <ul className="material-list">
                  {existingMaterials.map((material) => (
                    <li key={material.id}>
                      <span className="material-type" aria-hidden="true">
                        <MaterialIcon mimeType={material.mimeType} />
                      </span>
                      <span className="material-info">
                        <strong title={material.name}>{material.name}</strong>
                        <small>{formatBytes(material.size)} · 已归档</small>
                      </span>
                      <span className="material-actions">
                        {material.mimeType === "application/pdf" ||
                        material.mimeType.startsWith("image/") ? (
                          <Tooltip content="内置预览" relationship="label">
                            <Button
                              type="button"
                              appearance="subtle"
                              size="small"
                              icon={<Eye {...iconProps} />}
                              aria-label={`预览 ${material.name}`}
                              onClick={() => void showPreview(material)}
                            />
                          </Tooltip>
                        ) : null}
                        <Tooltip
                          content="使用系统程序打开"
                          relationship="label"
                        >
                          <Button
                            type="button"
                            appearance="subtle"
                            size="small"
                            icon={
                              material.mimeType === "application/pdf" ||
                              material.mimeType.startsWith("image/") ? (
                                <ExternalLink {...iconProps} />
                              ) : (
                                <FolderOpen {...iconProps} />
                              )
                            }
                            aria-label={`使用系统程序打开 ${material.name}`}
                            onClick={() => void openWithSystem(material.id)}
                          />
                        </Tooltip>
                        <Tooltip content="移除材料" relationship="label">
                          <Button
                            type="button"
                            appearance="subtle"
                            size="small"
                            icon={<Trash2 {...iconProps} />}
                            aria-label={`移除 ${material.name}`}
                            onClick={() => removeExisting(material.id)}
                          />
                        </Tooltip>
                      </span>
                    </li>
                  ))}
                  {pendingMaterials.map((material) => (
                    <li key={material.clientId}>
                      <span
                        className="material-type pending"
                        aria-hidden="true"
                      >
                        <MaterialIcon mimeType={material.mimeType} />
                      </span>
                      <span className="material-info">
                        <strong title={material.name}>{material.name}</strong>
                        <small>
                          {material.size
                            ? formatBytes(material.size)
                            : "保存时校验大小"}{" "}
                          · 待归档
                        </small>
                      </span>
                      <span className="material-actions">
                        <Tooltip content="移除材料" relationship="label">
                          <Button
                            type="button"
                            appearance="subtle"
                            size="small"
                            icon={<Trash2 {...iconProps} />}
                            aria-label={`移除 ${material.name}`}
                            onClick={() => {
                              setPendingMaterials((current) =>
                                current.filter(
                                  (item) => item.clientId !== material.clientId,
                                ),
                              );
                              setMaterialsDirty(true);
                            }}
                          />
                        </Tooltip>
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </form>
        </DrawerBody>

        <DrawerFooter className="drawer-footer">
          <Button
            appearance="secondary"
            disabled={saving}
            onClick={requestClose}
          >
            取消
          </Button>
          <Button
            appearance="primary"
            icon={<Save {...iconProps} />}
            disabled={saving}
            onClick={() =>
              document
                .querySelector<HTMLFormElement>("#record-editor-form")
                ?.requestSubmit()
            }
          >
            {saving ? "正在保存..." : "保存记录"}
          </Button>
        </DrawerFooter>
      </OverlayDrawer>

      <ConfirmDialog
        open={confirmHighScore}
        title="确认高分记录"
        confirmLabel="确认保存"
        busy={saving}
        onCancel={() => {
          setConfirmHighScore(false);
          pendingSubmit.current = null;
        }}
        onConfirm={() => {
          const values = pendingSubmit.current;
          setConfirmHighScore(false);
          pendingSubmit.current = null;
          if (values) void persist(values);
        }}
      >
        当前分数超过 1000 分，请确认分值和小数点填写正确。
      </ConfirmDialog>

      <Dialog
        open={previewOpen}
        onOpenChange={(_, data) => {
          if (!data.open) {
            setPreviewOpen(false);
            setPreview(null);
            setPreviewMaterialId("");
          }
        }}
      >
        <DialogSurface className="material-preview-surface">
          <DialogBody className="material-preview-body">
            <DialogTitle>证明材料预览</DialogTitle>
            <DialogContent className="material-preview-content">
              {previewLoading ? (
                <div className="material-preview-loading" role="status">
                  <span className="startup-loader" />
                  <span>正在加载材料...</span>
                </div>
              ) : preview?.mimeType.startsWith("image/") ? (
                <img src={preview.url} alt={preview.name} />
              ) : preview?.mimeType === "application/pdf" ? (
                <iframe src={preview.url} title={`预览 ${preview.name}`} />
              ) : null}
            </DialogContent>
            <DialogActions className="material-preview-actions">
              <span title={preview?.name}>{preview?.name}</span>
              <Button
                appearance="secondary"
                icon={<ExternalLink {...iconProps} />}
                disabled={!previewMaterialId || previewLoading}
                onClick={() => void openWithSystem(previewMaterialId)}
              >
                系统程序打开
              </Button>
              <Button
                appearance="primary"
                onClick={() => setPreviewOpen(false)}
              >
                关闭
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </>
  );
}
