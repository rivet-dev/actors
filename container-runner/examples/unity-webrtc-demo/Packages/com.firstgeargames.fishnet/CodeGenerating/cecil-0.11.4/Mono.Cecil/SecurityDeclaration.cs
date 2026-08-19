//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;
using System.Diagnostics;
using System.Threading;

namespace MonoFN.Cecil
{
    public enum SecurityAction : ushort
    {
        Request = 1,
        Demand = 2,
        Assert = 3,
        Deny = 4,
        PermitOnly = 5,
        LinkDemand = 6,
        InheritDemand = 7,
        RequestMinimum = 8,
        RequestOptional = 9,
        RequestRefuse = 10,
        PreJitGrant = 11,
        PreJitDeny = 12,
        NonCasDemand = 13,
        NonCasLinkDemand = 14,
        NonCasInheritance = 15
    }

    public interface ISecurityDeclarationProvider : IMetadataTokenProvider
    {
        bool HasSecurityDeclarations { get; }
        Collection<SecurityDeclaration> SecurityDeclarations { get; }
    }

    [DebuggerDisplay("{AttributeType}")]
    public sealed class SecurityAttribute : ICustomAttribute
    {
        internal Collection<CustomAttributeNamedArgument> fields;
        internal Collection<CustomAttributeNamedArgument> properties;
        public TypeReference AttributeType { get; set; }
        public bool HasFields
        {
            get { return !fields.IsNullOrEmpty(); }
        }
        public Collection<CustomAttributeNamedArgument> Fields
        {
            get
            {
                if (fields == null)
                    Interlocked.CompareExchange(ref fields, new(), null);

                return fields;
            }
        }
        public bool HasProperties
        {
            get { return !properties.IsNullOrEmpty(); }
        }
        public Collection<CustomAttributeNamedArgument> Properties
        {
            get
            {
                if (properties == null)
                    Interlocked.CompareExchange(ref properties, new(), null);

                return properties;
            }
        }

        public SecurityAttribute(TypeReference attributeType)
        {
            AttributeType = attributeType;
        }

        bool ICustomAttribute.HasConstructorArguments
        {
            get { return false; }
        }
        Collection<CustomAttributeArgument> ICustomAttribute.ConstructorArguments
        {
            get { throw new NotSupportedException(); }
        }
    }

    public sealed class SecurityDeclaration
    {
        internal readonly uint signature;
        private byte[] blob;
        private readonly ModuleDefinition module;
        internal bool resolved;
        internal Collection<SecurityAttribute> security_attributes;
        public SecurityAction Action { get; set; }
        public bool HasSecurityAttributes
        {
            get
            {
                Resolve();

                return !security_attributes.IsNullOrEmpty();
            }
        }
        public Collection<SecurityAttribute> SecurityAttributes
        {
            get
            {
                Resolve();

                if (security_attributes == null)
                    Interlocked.CompareExchange(ref security_attributes, new(), null);

                return security_attributes;
            }
        }
        internal bool HasImage
        {
            get { return module != null && module.HasImage; }
        }

        internal SecurityDeclaration(SecurityAction action, uint signature, ModuleDefinition module)
        {
            this.Action = action;
            this.signature = signature;
            this.module = module;
        }

        public SecurityDeclaration(SecurityAction action)
        {
            this.Action = action;
            resolved = true;
        }

        public SecurityDeclaration(SecurityAction action, byte[] blob)
        {
            this.Action = action;
            resolved = false;
            this.blob = blob;
        }

        public byte[] GetBlob()
        {
            if (blob != null)
                return blob;

            if (!HasImage || signature == 0)
                throw new NotSupportedException();

            return module.Read(ref blob, this, (declaration, reader) => reader.ReadSecurityDeclarationBlob(declaration.signature));
        }

        private void Resolve()
        {
            if (resolved || !HasImage)
                return;

            lock (module.SyncRoot)
            {
                if (resolved)
                    return;

                module.Read(this, (declaration, reader) => reader.ReadSecurityDeclarationSignature(declaration));
                resolved = true;
            }
        }
    }

    internal static partial class Mixin
    {
        public static bool GetHasSecurityDeclarations(this ISecurityDeclarationProvider self, ModuleDefinition module)
        {
            return module.HasImage() && module.Read(self, (provider, reader) => reader.HasSecurityDeclarations(provider));
        }

        public static Collection<SecurityDeclaration> GetSecurityDeclarations(this ISecurityDeclarationProvider self, ref Collection<SecurityDeclaration> variable, ModuleDefinition module)
        {
            if (module.HasImage)
                return module.Read(ref variable, self, (provider, reader) => reader.ReadSecurityDeclarations(provider));

            Interlocked.CompareExchange(ref variable, new(), null);
            return variable;
        }
    }
}