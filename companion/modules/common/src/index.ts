// Shared types for WebSocket messages and game data
// Will be populated as the project grows

export interface WsMessage {
  type: string;
  payload: unknown;
}
