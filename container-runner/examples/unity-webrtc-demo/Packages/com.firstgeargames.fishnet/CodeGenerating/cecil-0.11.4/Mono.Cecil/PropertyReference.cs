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

namespace MonoFN.Cecil
{
    public abstract class PropertyReference : MemberReference
    {
        public TypeReference PropertyType { get; set; }
        public abstract Collection<ParameterDefinition> Parameters { get; }

        internal PropertyReference(string name, TypeReference propertyType) : base(name)
        {
            Mixin.CheckType(propertyType, Mixin.Argument.propertyType);

            PropertyType = propertyType;
        }

        protected override IMemberDefinition ResolveDefinition()
        {
            return Resolve();
        }

        public new abstract PropertyDefinition Resolve();
    }
}