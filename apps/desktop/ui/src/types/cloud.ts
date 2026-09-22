/** Mirrors the Rust `CloudUserProfile` DTO, which itself mirrors the cloud backend's own UserProfile JSON shape. */
export interface CloudUserProfile {
  id: string;
  email: string;
  displayName: string;
  createdAt: string;
  /** True for an account somebody else created, until its owner sets a
   * password of their own. The backend refuses everything else while it
   * holds, so the app sends them to that screen rather than letting them
   * discover it one failed call at a time. */
  mustChangePassword: boolean;
}

/** The account a team lead just created, with the only readable copy of its
 * password. Shown once - nothing can produce it again. */
export interface CloudProvisionedMember {
  user: CloudUserProfile;
  temporaryPassword: string;
  roleAssigned: boolean;
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

/** One member allowed to see a shared application, joined to their account. */
export interface CloudApplicationMember {
  userId: string;
  email: string;
  displayName: string;
  grantedAt: string;
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
