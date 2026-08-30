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

export interface CloudRole {
  id: string;
  teamId: string;
  name: string;
  description: string | null;
  isSystem: boolean;
  createdAt: string;
}

export interface CloudRoleWithPermissions extends CloudRole {
  permissions: string[];
}

export interface CloudServer {
  id: string;
  teamId: string;
  name: string;
  host: string;
  sshPort: number;
  username: string | null;
  createdAt: string;
}
