import {
  Header,
  HeaderName,
  HeaderGlobalBar,
  HeaderGlobalAction,
} from "@carbon/react";
import { FolderOpen, Home, Information, Renew } from "@carbon/icons-react";

interface AppHeaderProps {
  /** Whether an image is open (explorer view) — gates the explorer actions. */
  imageOpen: boolean;
  infoOpen: boolean;
  onOpen: () => void;
  onRefresh: () => void;
  onToggleInfo: () => void;
  /** Close the current image and return to the start page. */
  onCloseImage: () => void;
}

/** The fixed Carbon UI-Shell header: product name plus global actions for
 *  picking a container, refreshing, toggling the details panel, and returning
 *  to the start page. */
export default function AppHeader({
  imageOpen,
  infoOpen,
  onOpen,
  onRefresh,
  onToggleInfo,
  onCloseImage,
}: AppHeaderProps) {
  return (
    <Header aria-label="apfsRelic file explorer">
      <HeaderName href="#" prefix="">
        🗄️&nbsp;apfsRelic
      </HeaderName>
      <HeaderGlobalBar>
        <HeaderGlobalAction
          aria-label="Open disk image…"
          onClick={onOpen}
          tooltipAlignment={imageOpen ? "center" : "end"}
        >
          <FolderOpen size={20} />
        </HeaderGlobalAction>
        {imageOpen && (
          <>
            <HeaderGlobalAction
              aria-label="Refresh"
              onClick={onRefresh}
              tooltipAlignment="center"
            >
              <Renew size={20} />
            </HeaderGlobalAction>
            <HeaderGlobalAction
              aria-label={infoOpen ? "Hide details panel" : "Show details panel"}
              isActive={infoOpen}
              onClick={onToggleInfo}
              tooltipAlignment="center"
            >
              <Information size={20} />
            </HeaderGlobalAction>
            <HeaderGlobalAction
              aria-label="Close image"
              onClick={onCloseImage}
              tooltipAlignment="end"
            >
              <Home size={20} />
            </HeaderGlobalAction>
          </>
        )}
      </HeaderGlobalBar>
    </Header>
  );
}
