// Official Craft app icons are used only where upstream licenses them (MIT/Apache-2.0); see
// ASSETS.md. Everything else, including ArtCraft (trademarked logo), gets a generic Lucide icon.
import {
  Aperture,
  AudioWaveform,
  Clapperboard,
  DraftingCompass,
  FileText,
  Image,
  LayoutTemplate,
  Palette,
  PenTool,
  Presentation,
  Sheet,
  Sparkles,
  Type,
  Box,
  type LucideIcon,
} from "lucide-react";

const ICONS: Record<string, LucideIcon> = {
  photocraft: Image,
  vectorcraft: PenTool,
  filmcraft: Clapperboard,
  lightcraft: Aperture,
  pdfcraft: FileText,
  effectcraft: Sparkles,
  designcraft: LayoutTemplate,
  soundcraft: AudioWaveform,
  wordcraft: Type,
  gridcraft: Sheet,
  deckcraft: Presentation,
  cadcraft: DraftingCompass,
  artcraft: Palette,
};

const HUES: Record<string, number> = {
  photocraft: 205,
  vectorcraft: 25,
  filmcraft: 265,
  lightcraft: 45,
  pdfcraft: 0,
  effectcraft: 290,
  designcraft: 330,
  soundcraft: 165,
  wordcraft: 220,
  gridcraft: 140,
  deckcraft: 15,
  cadcraft: 190,
  artcraft: 310,
};

const OFFICIAL: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob<string>("../assets/app-icons/*.png", {
      eager: true,
      query: "?url",
      import: "default",
    }),
  ).map(([path, url]) => [path.split("/").pop()!.replace(".png", ""), url]),
);

export function hasOfficialIcon(appId: string): boolean {
  return appId in OFFICIAL;
}

export function AppIcon({ appId, size = 44 }: { appId: string; size?: number }) {
  const official = OFFICIAL[appId];
  if (official) {
    return (
      <img
        className="app-icon app-icon-img"
        src={official}
        alt=""
        aria-hidden="true"
        width={size}
        height={size}
        draggable={false}
      />
    );
  }
  const Icon = ICONS[appId] ?? Box;
  const hue = HUES[appId] ?? 230;
  return (
    <div
      className="app-icon"
      aria-hidden="true"
      style={{
        width: size,
        height: size,
        background: `linear-gradient(135deg, hsl(${hue} 70% 55%), hsl(${(hue + 40) % 360} 65% 40%))`,
      }}
    >
      <Icon size={size * 0.5} strokeWidth={2} color="white" />
    </div>
  );
}
