// Generate 156 unique image prompts for role cards
// Style: realistic human work positions, varied colors/art styles, NO text in images
// Output: role-prompts.json

const fs = require('fs');
const roles = require('../roles/roles-en.json');

// 20+ art styles (realistic, not sci-fi)
const STYLES = [
  'oil painting style, rich warm tones',
  'watercolor illustration, soft pastel colors',
  'digital painting, cool blue and teal palette',
  'cinematic photography style, dramatic lighting',
  'flat vector illustration, bold primary colors',
  'charcoal sketch style with amber highlights',
  'low-poly geometric art, muted earth tones',
  'vintage poster style, sepia and gold tones',
  'isometric illustration, vibrant gradient colors',
  'pencil portrait with subtle color wash',
  'pop art style, high contrast neon colors',
  'editorial illustration, elegant muted palette',
  'gouache painting, warm sunset palette',
  'line art with watercolor fill, spring colors',
  'retro 80s style, pink and purple gradients',
  'minimalist silhouette, deep navy and gold',
  'impressionist style, dappled light effect',
  'graphic novel style, strong shadows and highlights',
  'art deco style, geometric gold and black',
  'Japanese ukiyo-e inspired, soft ink wash',
  'Nordic design style, clean whites and forest green',
  'stained glass mosaic style, jewel tones',
  'chiaroscuro style, dramatic dark background',
  'paper cut-out collage style, layered textures',
  'blueprint style, white lines on deep blue',
  'mosaic tile art, Mediterranean colors',
  'woodblock print style, earthy brown tones',
  'pointillism style, vivid dots of color',
];

// Role-specific scene descriptions
function getRoleScene(name, category) {
  const n = name.toLowerCase();

  // Engineering
  if (n.includes('frontend') || n.includes('ui ')) return 'a developer working on user interface designs on multiple screens';
  if (n.includes('backend') || n.includes('server')) return 'an engineer monitoring server infrastructure dashboards';
  if (n.includes('fullstack')) return 'a developer with both design mockups and code on dual monitors';
  if (n.includes('devops') || n.includes('infrastructure')) return 'an engineer in a server room with monitoring displays';
  if (n.includes('security') || n.includes('blockchain')) return 'a security professional analyzing code patterns on screen';
  if (n.includes('data scientist') || n.includes('data engineer')) return 'a data professional examining charts and visualizations';
  if (n.includes('database')) return 'a professional working with database schema diagrams';
  if (n.includes('mobile') || n.includes('ios') || n.includes('android')) return 'a developer testing apps on mobile devices';
  if (n.includes('embedded') || n.includes('firmware') || n.includes('hardware')) return 'an engineer working with circuit boards and microcontrollers';
  if (n.includes('ai ') || n.includes('machine learning') || n.includes('ml ')) return 'a researcher reviewing neural network diagrams';
  if (n.includes('api')) return 'a developer designing API architecture on a whiteboard';
  if (n.includes('git') || n.includes('version')) return 'a developer reviewing code branches on screen';
  if (n.includes('code review')) return 'a senior developer reviewing colleague code on screen';
  if (n.includes('architect')) return 'a technical architect drawing system diagrams';
  if (n.includes('performance') || n.includes('benchmark')) return 'an engineer analyzing performance graphs and metrics';
  if (n.includes('documentation') || n.includes('technical writ')) return 'a writer working on structured documentation';
  if (n.includes('solidity') || n.includes('smart contract') || n.includes('zk')) return 'a blockchain developer working with smart contract code';
  if (n.includes('rust') || n.includes('systems')) return 'a systems programmer working on low-level code';
  if (n.includes('cloud') || n.includes('aws') || n.includes('kubernetes')) return 'an engineer managing cloud infrastructure dashboards';
  if (n.includes('electron') || n.includes('desktop')) return 'a developer building desktop application interfaces';
  if (n.includes('feishu') || n.includes('lark') || n.includes('wechat') || n.includes('mini')) return 'a developer creating mini-program interfaces on tablet';
  if (n.includes('refactor') || n.includes('migration')) return 'a developer reorganizing code structure on screen';
  if (n.includes('debug') || n.includes('troubleshoot')) return 'a developer debugging code with console output';

  // Design
  if (n.includes('ux') || n.includes('user experience')) return 'a designer creating wireframes and user flows';
  if (n.includes('brand') || n.includes('visual')) return 'a designer working on brand identity materials';
  if (n.includes('motion') || n.includes('animation')) return 'a designer creating motion graphics on a large display';
  if (n.includes('design system')) return 'a designer organizing component libraries and style guides';
  if (n.includes('accessibility')) return 'a specialist testing interfaces with assistive technology';

  // Marketing
  if (n.includes('seo')) return 'a marketer analyzing search rankings and keyword data';
  if (n.includes('content') || n.includes('copywrite') || n.includes('writer')) return 'a writer crafting compelling content at a modern desk';
  if (n.includes('social media')) return 'a strategist managing multiple social media dashboards';
  if (n.includes('email') || n.includes('newsletter')) return 'a marketer designing email campaign templates';
  if (n.includes('analytics') || n.includes('growth')) return 'a growth analyst reviewing conversion funnels';
  if (n.includes('market research') || n.includes('competitive')) return 'a researcher analyzing market data and competitor profiles';
  if (n.includes('influencer')) return 'a marketing manager coordinating influencer partnerships';

  // Product
  if (n.includes('product manager') || n.includes('product owner')) return 'a product manager prioritizing features on a kanban board';
  if (n.includes('prototype') || n.includes('rapid')) return 'a designer rapidly building interactive prototypes';
  if (n.includes('onboarding')) return 'a specialist designing user onboarding flow screens';
  if (n.includes('pricing') || n.includes('monetization')) return 'a strategist analyzing pricing models and revenue charts';
  if (n.includes('roadmap')) return 'a product lead mapping out product timeline on whiteboard';
  if (n.includes('feedback') || n.includes('customer')) return 'a professional reviewing customer feedback surveys';

  // Sales
  if (n.includes('sales') || n.includes('presales')) return 'a sales professional presenting proposals to clients';
  if (n.includes('crm') || n.includes('pipeline')) return 'a sales manager reviewing CRM pipeline dashboard';
  if (n.includes('pitch') || n.includes('proposal')) return 'a professional crafting a business pitch presentation';
  if (n.includes('outreach') || n.includes('prospecting')) return 'a salesperson conducting outreach calls at desk';
  if (n.includes('negotiation') || n.includes('deal')) return 'a deal closer in a negotiation meeting room';

  // Project Management
  if (n.includes('agile') || n.includes('scrum')) return 'a scrum master facilitating a team standup meeting';
  if (n.includes('jira') || n.includes('workflow')) return 'a project manager organizing tasks on a workflow board';
  if (n.includes('producer') || n.includes('studio')) return 'a studio producer coordinating team schedules';
  if (n.includes('project manager') || n.includes('shepherd')) return 'a project manager reviewing Gantt charts and timelines';
  if (n.includes('experiment') || n.includes('tracker')) return 'a professional tracking experiments on a data dashboard';

  // Testing
  if (n.includes('test') || n.includes('qa')) return 'a QA engineer running test suites on multiple screens';
  if (n.includes('tool evaluator')) return 'a professional comparing different software tools side by side';

  // Support
  if (n.includes('support') || n.includes('responder')) return 'a support specialist helping users through a headset';
  if (n.includes('finance') || n.includes('accounts')) return 'a finance professional reviewing spreadsheets and reports';
  if (n.includes('legal') || n.includes('compliance')) return 'a compliance officer reviewing regulatory documents';
  if (n.includes('executive summary') || n.includes('report')) return 'a professional preparing executive summary reports';

  // Game Development
  if (n.includes('game design')) return 'a game designer sketching level layouts and mechanics';
  if (n.includes('level design')) return 'a designer building 3D game environments';
  if (n.includes('narrative') || n.includes('story')) return 'a narrative writer crafting story arcs on a board';
  if (n.includes('audio') || n.includes('sound')) return 'a sound engineer working in a recording studio';
  if (n.includes('shader') || n.includes('vfx') || n.includes('visual effect')) return 'an artist creating visual effects on a high-end workstation';
  if (n.includes('unity') || n.includes('unreal') || n.includes('godot')) return 'a game developer working in a game engine editor';
  if (n.includes('roblox')) return 'a developer creating colorful game experiences';
  if (n.includes('blender') || n.includes('3d') || n.includes('technical artist')) return 'a 3D artist modeling assets in a creative workspace';
  if (n.includes('multiplayer') || n.includes('network')) return 'a developer testing multiplayer game connections';
  if (n.includes('avatar')) return 'a designer creating character customization systems';

  // Specialized
  if (n.includes('recruit')) return 'a recruiter reviewing candidate profiles and resumes';
  if (n.includes('training') || n.includes('education')) return 'a training designer creating learning materials';
  if (n.includes('healthcare')) return 'a healthcare professional reviewing medical documents';
  if (n.includes('supply chain') || n.includes('logistics')) return 'a logistics manager tracking shipment routes';
  if (n.includes('govern') || n.includes('policy')) return 'a government consultant reviewing policy documents';
  if (n.includes('cultural') || n.includes('international')) return 'a consultant studying cultural business practices';
  if (n.includes('french') || n.includes('korean')) return 'a business navigator bridging international markets';
  if (n.includes('advocate')) return 'a developer advocate presenting at a tech conference';
  if (n.includes('document generator')) return 'a professional generating structured documents';
  if (n.includes('salesforce')) return 'an architect designing enterprise CRM solutions';
  if (n.includes('orchestrat')) return 'a professional coordinating multiple automated workflows';
  if (n.includes('identity') || n.includes('trust')) return 'a security architect designing identity systems';
  if (n.includes('study abroad') || n.includes('advisor')) return 'an advisor helping students plan academic journeys';
  if (n.includes('model qa')) return 'a specialist testing and validating AI model outputs';
  if (n.includes('mcp builder') || n.includes('lsp')) return 'a developer building integration tools and protocols';

  // Academic
  if (n.includes('anthropolog')) return 'an anthropologist studying cultural artifacts';
  if (n.includes('geograph')) return 'a geographer analyzing maps and terrain data';
  if (n.includes('historian')) return 'a historian researching in an archive library';
  if (n.includes('narratolog')) return 'a scholar analyzing narrative structures in texts';
  if (n.includes('psycholog')) return 'a psychologist conducting research analysis';

  // Paid Media
  if (n.includes('ppc') || n.includes('search')) return 'a PPC specialist optimizing ad campaign bids';
  if (n.includes('programmatic') || n.includes('display')) return 'a media buyer managing programmatic ad platforms';
  if (n.includes('creative strat') || n.includes('ad creative')) return 'a creative strategist reviewing ad designs';
  if (n.includes('tracking') || n.includes('measurement')) return 'a tracking specialist implementing analytics tags';
  if (n.includes('audit')) return 'an auditor reviewing campaign performance metrics';

  // Spatial Computing
  if (n.includes('spatial') || n.includes('visionos') || n.includes('xr') || n.includes('vr') || n.includes('ar')) return 'a developer prototyping immersive spatial interfaces';
  if (n.includes('metal') || n.includes('macos')) return 'a developer building high-performance graphics applications';
  if (n.includes('terminal') || n.includes('cli')) return 'a developer working in terminal with command-line tools';
  if (n.includes('cockpit')) return 'a specialist designing cockpit interaction interfaces';

  // Generic fallbacks by category
  const catMap = {
    'engineering': 'a software engineer working at a modern workstation',
    'design': 'a designer creating visual concepts at a creative desk',
    'marketing': 'a marketing professional analyzing campaign strategies',
    'product': 'a product professional mapping user journeys',
    'sales': 'a sales professional reviewing business metrics',
    'project-management': 'a project manager coordinating team efforts',
    'testing': 'a QA professional testing software quality',
    'support': 'a support professional helping users',
    'game-development': 'a game developer in a creative game studio',
    'specialized': 'a specialist working in a professional office',
    'academic': 'a researcher studying in a university setting',
    'paid-media': 'a media professional managing advertising campaigns',
    'spatial-computing': 'a developer working with spatial computing technology',
  };

  return catMap[category] || 'a professional working at a modern office desk';
}

// Build prompts
const prompts = roles.roles.map((role, i) => {
  const style = STYLES[i % STYLES.length];
  const scene = getRoleScene(role.name, role.category);

  return {
    index: i + 1,
    id: role.id,
    name: role.name,
    category: role.category,
    filename: `${i + 1}.jpg`,
    prompt: `Portrait of ${scene}. ${style}. Professional workplace setting, 5:8 vertical aspect ratio, no text or letters in the image, no watermark. High quality, detailed.`,
  };
});

// Write output
const output = JSON.stringify(prompts, null, 2);
fs.writeFileSync('d:/Echobird/tmp/role-prompts.json', output, 'utf8');

// Also write a simple text file for easy copy-paste
const textLines = prompts.map(p => `#${p.index} [${p.category}] ${p.name}\n${p.prompt}\n`);
fs.writeFileSync('d:/Echobird/tmp/role-prompts.txt', textLines.join('\n---\n\n'), 'utf8');

console.log(`Generated ${prompts.length} prompts`);
console.log(`JSON: d:/Echobird/tmp/role-prompts.json`);
console.log(`TXT:  d:/Echobird/tmp/role-prompts.txt`);
