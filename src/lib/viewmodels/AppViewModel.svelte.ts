export type DrawerId = "devices" | "settings";

/**
 * Navigation and overlay state for the app shell.
 * Presentation-only; no Tauri, no device rules.
 */
export class AppViewModel {
  drawer = $state<DrawerId | null>(null);
  authHelpOpen = $state(false);

  openDrawer(drawer: DrawerId): void {
    this.drawer = drawer;
  }

  closeDrawer(): void {
    this.drawer = null;
  }

  toggleDrawer(drawer: DrawerId): void {
    this.drawer = this.drawer === drawer ? null : drawer;
  }

  openAuthHelp(): void {
    this.authHelpOpen = true;
  }

  closeAuthHelp(): void {
    this.authHelpOpen = false;
  }

  /** Esc priority for M0a: auth modal, then drawer. Returns true when consumed. */
  handleEscape(): boolean {
    if (this.authHelpOpen) {
      this.authHelpOpen = false;
      return true;
    }
    if (this.drawer !== null) {
      this.drawer = null;
      return true;
    }

    return false;
  }
}
