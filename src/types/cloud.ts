/** Mirrors the Rust `CloudUserProfile` DTO, which itself mirrors the cloud backend's own UserProfile JSON shape. */
export interface CloudUserProfile {
  id: string;
  email: string;
  displayName: string;
  createdAt: string;
}

export interface CloudSessionInfo {
  user: CloudUserProfile;
}

export interface CloudTeam {
  id: string;
  name: string;
  ownerId: string;
  createdAt: string;
}

export interface CloudTeamMember {
  userId: string;
  email: string;
  displayName: string;
  joinedAt: string;
  isOwner: boolean;
}
