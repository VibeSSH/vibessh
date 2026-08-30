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

export type InvitationStatus = "pending" | "accepted" | "declined" | "revoked" | "expired";

export interface CloudInvitation {
  id: string;
  teamId: string;
  email: string;
  roleId: string | null;
  status: InvitationStatus;
  invitedBy: string | null;
  createdAt: string;
  expiresAt: string;
}

/** Only ever returned once, right after creating an invitation - see cloudCreateInvitation. */
export interface CloudCreatedInvitation extends CloudInvitation {
  token: string;
}

export interface CloudAuditEvent {
  id: string;
  action: string;
  targetType: string;
  targetId: string | null;
  result: string;
  metadata: unknown;
  createdAt: string;
  actorId: string | null;
  actorEmail: string | null;
  actorDisplayName: string | null;
}
