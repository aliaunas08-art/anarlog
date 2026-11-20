import { cn } from "@anlg/utils";

// Brand marks as static SVG files in public/assets/brand-icons, rendered once
// from @lobehub/icons 3.0.2 (MIT). The package was removed because it pulled
// in @lobehub/ui, and through it @splinetool/runtime (no licence) and
// @giscus/react. Coloured marks keep their colours; monochrome ones are drawn
// black and follow the theme through --provider-brand-filter.
const BRAND_ICONS = {
  AlibabaCloud: { file: "alibaba-cloud.svg", colored: true },
  Apple: { file: "apple.svg", colored: false },
  AssemblyAI: { file: "assembly-ai.svg", colored: true },
  Aws: { file: "aws.svg", colored: true },
  Azure: { file: "azure.svg", colored: true },
  Bedrock: { file: "bedrock.svg", colored: true },
  Claude: { file: "claude.svg", colored: true },
  Cohere: { file: "cohere.svg", colored: true },
  Cursor: { file: "cursor.svg", colored: false },
  ElevenLabs: { file: "eleven-labs.svg", colored: false },
  Figma: { file: "figma.svg", colored: true },
  Gemini: { file: "gemini.svg", colored: true },
  Github: { file: "github.svg", colored: false },
  Google: { file: "google.svg", colored: true },
  GoogleCloud: { file: "google-cloud.svg", colored: true },
  Groq: { file: "groq.svg", colored: true },
  Mistral: { file: "mistral.svg", colored: true },
  Notion: { file: "notion.svg", colored: false },
  Nvidia: { file: "nvidia.svg", colored: true },
  OpenAI: { file: "open-ai.svg", colored: false },
  OpenRouter: { file: "open-router.svg", colored: true },
  SiliconCloud: { file: "silicon-cloud.svg", colored: true },
  Together: { file: "together.svg", colored: true },
  WorkersAI: { file: "workers-ai.svg", colored: true },
  XAI: { file: "xai.svg", colored: false },
  ZAI: { file: "zai.svg", colored: false },
} as const;

export type BrandIconName = keyof typeof BRAND_ICONS;

export function brandIconSrc(name: BrandIconName): string {
  return `/assets/brand-icons/${BRAND_ICONS[name].file}`;
}

export function isColoredBrandIcon(name: BrandIconName): boolean {
  return BRAND_ICONS[name].colored;
}

export function BrandIcon({
  name,
  className,
  size = 16,
}: {
  name: BrandIconName;
  className?: string;
  size?: number;
}) {
  const colored = isColoredBrandIcon(name);
  return (
    <img
      src={brandIconSrc(name)}
      alt={name}
      width={size}
      height={size}
      draggable={false}
      data-slot={colored ? "provider-color-icon" : "provider-brand-icon"}
      className={cn([
        "shrink-0 object-contain",
        !colored && "[filter:var(--provider-brand-filter)]",
        className,
      ])}
    />
  );
}
