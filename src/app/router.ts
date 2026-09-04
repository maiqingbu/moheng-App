import { createRouter, createWebHistory } from 'vue-router'
import ProjectLibrary from '../ui/pages/ProjectLibrary.vue'
import CreationLaunch from '../ui/pages/CreationLaunch.vue'
import ProjectOverview from '../ui/pages/ProjectOverview.vue'
import MarketStrategy from '../ui/pages/MarketStrategy.vue'
import PitchReview from '../ui/pages/PitchReview.vue'
import StorySetup from '../ui/pages/StorySetup.vue'
import OpeningReady from '../ui/pages/OpeningReady.vue'
import ProjectAssistant from '../ui/pages/ProjectAssistant.vue'
import ProductionCenter from '../ui/pages/ProductionCenter.vue'
import VolumeManager from '../ui/pages/VolumeManager.vue'
import VolumeWorkspace from '../ui/pages/VolumeWorkspace.vue'
import DecisionInbox from '../ui/pages/DecisionInbox.vue'
import ManuscriptReader from '../ui/pages/ManuscriptReader.vue'
import CanonGraph from '../ui/pages/CanonGraph.vue'
import HistoryCenter from '../ui/pages/HistoryCenter.vue'
import LongFormHealth from '../ui/pages/LongFormHealth.vue'
import SettingsPage from '../ui/pages/SettingsPage.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', redirect: '/projects' },
    { path: '/projects', name: 'projects', component: ProjectLibrary, meta: { scope: 'library' } },
    { path: '/projects/new', name: 'new-project', component: CreationLaunch, meta: { scope: 'library' } },
    { path: '/projects/:id', name: 'project-overview', component: ProjectOverview, meta: { scope: 'project' } },
    { path: '/projects/:id/market', name: 'market', component: MarketStrategy, meta: { scope: 'project' } },
    { path: '/projects/:id/directions', name: 'directions', component: PitchReview, meta: { scope: 'project' } },
    { path: '/projects/:id/setup', name: 'story-setup', component: StorySetup, meta: { scope: 'project' } },
    { path: '/projects/:id/opening', name: 'opening-ready', component: OpeningReady, meta: { scope: 'project' } },
    { path: '/projects/:id/assistant', name: 'assistant', component: ProjectAssistant, meta: { scope: 'project' } },
    { path: '/projects/:id/production', name: 'production', component: ProductionCenter, meta: { scope: 'project' } },
    { path: '/projects/:id/volumes', name: 'volumes', component: VolumeManager, meta: { scope: 'project' } },
    { path: '/projects/:id/volumes/:volumeId', name: 'volume-workspace', component: VolumeWorkspace, meta: { scope: 'project' } },
    { path: '/projects/:id/decisions', name: 'decisions', component: DecisionInbox, meta: { scope: 'project' } },
    { path: '/projects/:id/read', name: 'read', component: ManuscriptReader, meta: { scope: 'project' } },
    { path: '/projects/:id/graph', name: 'graph', component: CanonGraph, meta: { scope: 'project' } },
    { path: '/projects/:id/health', name: 'health', component: LongFormHealth, meta: { scope: 'project', availability: 'available' } },
    { path: '/projects/:id/history', name: 'history', component: HistoryCenter, meta: { scope: 'project', availability: 'available' } },
    { path: '/projects/:id/settings', name: 'project-settings', component: SettingsPage, meta: { scope: 'project' } },
    { path: '/settings', name: 'settings', component: SettingsPage, meta: { scope: 'global' } },
    { path: '/:pathMatch(.*)*', redirect: '/projects' },
  ],
  scrollBehavior: () => ({ top: 0 }),
})
